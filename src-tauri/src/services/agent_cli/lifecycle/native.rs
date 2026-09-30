use super::{
    filesystem::{changed, owned_writable_directory, FileStamp},
    npm::{self, NpmRuntime},
    planning::{unavailable, LifecycleContext, LifecycleExecution, RunEvidence, Verification},
    service::map_mutation_error,
};
use crate::{
    models::*,
    services::agent_cli::environment::{
        config_document::{self, ConfigDocumentFormat},
        mutation::{
            execution::{ExactCliCommand, ExecutableStamp},
            GuardedDirectory,
        },
    },
};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub(super) const UPGRADE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CLAUDE_RELEASES: &str = "https://downloads.claude.ai/claude-code-releases";
const GROK_RELEASES: &str = "https://x.ai/cli";

#[derive(Clone)]
pub(super) enum Mechanism {
    Homebrew(Box<super::homebrew::HomebrewInstallation>),
    Npm {
        prefix: PathBuf,
        runtime: Box<NpmRuntime>,
        package: Box<FileStamp>,
    },
    Vendor {
        directory: PathBuf,
        launcher: PathBuf,
        track: String,
        receipts: Vec<FileStamp>,
        absent_receipts: Vec<PathBuf>,
    },
}

impl Mechanism {
    pub(super) fn directory(&self) -> &Path {
        match self {
            Self::Npm { prefix, .. } => prefix,
            Self::Homebrew(installation) => &installation.prefix,
            Self::Vendor { directory, .. } => directory,
        }
    }

    pub(super) fn release_url(&self, kind: AgentCliKind) -> Option<String> {
        let Self::Vendor { track, .. } = self else {
            return None;
        };
        match kind {
            AgentCliKind::ClaudeCode => Some(format!("{CLAUDE_RELEASES}/{track}")),
            AgentCliKind::Grok => Some(format!("{GROK_RELEASES}/{track}")),
            _ => None,
        }
    }

    pub(super) fn signature(&self) -> String {
        match self {
            Self::Homebrew(installation) => installation.signature(),
            Self::Npm {
                prefix,
                runtime,
                package,
            } => format!(
                "npm\0{}\0{}\0{}",
                prefix.display(),
                runtime.signature(),
                package.signature()
            ),
            Self::Vendor {
                directory,
                launcher,
                track,
                receipts,
                absent_receipts,
            } => format!(
                "vendor\0{}\0{}\0{}\0{}\0{}",
                directory.display(),
                launcher.display(),
                track,
                receipts
                    .iter()
                    .map(FileStamp::signature)
                    .collect::<Vec<_>>()
                    .join("\0"),
                absent_receipts
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\0")
            ),
        }
    }
}

/// A native binary's signature alone does not prove where `install`/`update`
/// writes. Require the exact documented launcher tree and installer receipt.
pub(super) fn vendor_installation(
    context: &LifecycleContext,
    installation: &AgentInstallation,
) -> Result<Mechanism, AgentLifecycleError> {
    if !cfg!(target_os = "macos") {
        return Err(unavailable(
            AgentLifecycleUnavailableReason::UnsupportedPlatform,
        ));
    }
    if installation.distribution != AgentCliDistribution::VendorNative {
        return Err(unavailable(
            AgentLifecycleUnavailableReason::ProvenanceUnverified,
        ));
    }
    let version = installation
        .installed_version
        .as_deref()
        .and_then(npm::normalized_version)
        .ok_or_else(changed)?;
    let canonical = installation
        .executable_identity
        .as_ref()
        .map(|identity| PathBuf::from(&identity.canonical_path))
        .ok_or_else(changed)?;
    match installation.agent_kind {
        AgentCliKind::ClaudeCode => {
            let directory = context.home.join(".local/share/claude");
            let launcher = context.home.join(".local/bin/claude");
            if canonical != directory.join("versions").join(&version)
                || fs::canonicalize(&launcher).ok().as_ref() != Some(&canonical)
            {
                return Err(unavailable(
                    AgentLifecycleUnavailableReason::ProvenanceUnverified,
                ));
            }
            let (receipt, bytes) = FileStamp::read(&context.home.join(".claude.json"), 512 * 1024)?;
            let document =
                config_document::parse(&bytes, ConfigDocumentFormat::Json).ok_or_else(changed)?;
            if document
                .get("installMethod")
                .and_then(|value| value.as_str())
                != Some("native")
            {
                return Err(unavailable(
                    AgentLifecycleUnavailableReason::ProvenanceUnverified,
                ));
            }
            let mut receipts = vec![receipt];
            let mut absent_receipts = Vec::new();
            let mut track = "latest".to_owned();
            // Managed policy has higher priority. Never infer it from npm tags.
            for settings in [
                context.home.join(".claude/settings.json"),
                PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json"),
            ] {
                match fs::symlink_metadata(&settings) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        absent_receipts.push(settings)
                    }
                    Ok(_) => {
                        let (stamp, bytes) = FileStamp::read(&settings, 512 * 1024)?;
                        let document = config_document::parse(&bytes, ConfigDocumentFormat::Json)
                            .ok_or_else(changed)?;
                        if let Some(value) = document.get("autoUpdatesChannel") {
                            track = value
                                .as_str()
                                .filter(|value| matches!(*value, "latest" | "stable"))
                                .ok_or_else(|| {
                                    unavailable(AgentLifecycleUnavailableReason::UnsupportedChannel)
                                })?
                                .to_owned();
                        }
                        receipts.push(stamp);
                    }
                    Err(_) => return Err(changed()),
                }
            }
            Ok(Mechanism::Vendor {
                directory,
                launcher,
                track,
                receipts,
                absent_receipts,
            })
        }
        AgentCliKind::Grok => {
            let directory = context.home.join(".grok");
            let launcher = directory.join("bin/grok");
            if fs::canonicalize(&launcher).ok().as_ref() != Some(&canonical)
                || !(canonical.starts_with(directory.join("downloads"))
                    || canonical.starts_with(directory.join("bin")))
            {
                return Err(unavailable(
                    AgentLifecycleUnavailableReason::ProvenanceUnverified,
                ));
            }
            let (receipt, bytes) = FileStamp::read(&directory.join("config.toml"), 512 * 1024)?;
            let document =
                config_document::parse(&bytes, ConfigDocumentFormat::Toml).ok_or_else(changed)?;
            if document
                .pointer("/cli/installer")
                .and_then(|value| value.as_str())
                != Some("internal")
            {
                return Err(unavailable(
                    AgentLifecycleUnavailableReason::UnsupportedChannel,
                ));
            }
            let track = match document.pointer("/cli/channel") {
                None => "stable",
                Some(value) => value
                    .as_str()
                    .filter(|value| matches!(*value, "stable" | "alpha"))
                    .ok_or_else(|| {
                        unavailable(AgentLifecycleUnavailableReason::UnsupportedChannel)
                    })?,
            }
            .to_owned();
            Ok(Mechanism::Vendor {
                directory,
                launcher,
                track,
                receipts: vec![receipt],
                absent_receipts: Vec::new(),
            })
        }
        _ => Err(unavailable(
            AgentLifecycleUnavailableReason::UnsupportedChannel,
        )),
    }
}

/// Probe the selected executable instead of imposing a sampled version floor.
/// Only planning calls this; browsing versions never runs upgrade help commands.
pub(super) fn validate_vendor_upgrade(
    context: &LifecycleContext,
    installation: &AgentInstallation,
    mechanism: &Mechanism,
) -> Result<(), AgentLifecycleError> {
    let (subcommand, required) = match installation.agent_kind {
        AgentCliKind::ClaudeCode => ("update", "--help"),
        AgentCliKind::Grok => ("update", "--help"),
        _ => {
            return Err(unavailable(
                AgentLifecycleUnavailableReason::UnsupportedChannel,
            ))
        }
    };
    let mut environment = npm::environment(&context.home);
    if installation.agent_kind == AgentCliKind::Grok {
        environment.push((
            OsString::from("GROK_HOME"),
            mechanism.directory().as_os_str().to_owned(),
        ));
    }
    let command = ExactCliCommand::new(
        installation,
        vec![subcommand.to_owned(), "--help".to_owned()],
        &context.home,
        environment,
    )
    .map_err(|error| {
        let mut failure = map_mutation_error(error);
        failure.message = format!(
            "检查原生升级能力（{subcommand} --help）失败：{}",
            failure.message
        );
        failure
    })?;
    let output = command
        .probe_output(&context.settings, Duration::from_secs(5))
        .map_err(|error| {
            let mut failure = map_mutation_error(error);
            failure.message = format!(
                "检查原生升级能力（{subcommand} --help）失败：{}",
                failure.message
            );
            failure
        })?;
    let help = format!("{}\n{}", output.stdout, output.stderr).to_ascii_lowercase();
    if output.timed_out
        || output.stdout_truncated
        || output.stderr_truncated
        || !output.status.is_some_and(|status| status.success())
        || !help.contains(subcommand)
        || !help.contains(required)
    {
        let mut error = unavailable(AgentLifecycleUnavailableReason::UnsupportedChannel);
        let cause = if output.timed_out {
            "命令超时".to_owned()
        } else if output.stdout_truncated || output.stderr_truncated {
            "帮助输出超过读取上限".to_owned()
        } else if !output.status.is_some_and(|status| status.success()) {
            format!(
                "退出码 {}",
                output
                    .status
                    .and_then(|status| status.code())
                    .map_or_else(|| "未返回".to_owned(), |code| code.to_string())
            )
        } else {
            format!("帮助中未找到所需参数 {required}")
        };
        error.message = format!(
            "原生升级能力检查失败（{subcommand} --help）：{cause}。{}",
            output.stderr.chars().take(2000).collect::<String>()
        );
        return Err(error);
    }
    Ok(())
}

pub(super) struct NativeExecution {
    context: LifecycleContext,
    kind: AgentCliKind,
    mechanism: Mechanism,
    directory: GuardedDirectory,
    related_directories: Vec<GuardedDirectory>,
    selected_executable: ExecutableStamp,
    command: ExactCliCommand,
    npm_script: Option<ExecutableStamp>,
    previous_version: Option<String>,
    upgrade_attempted: AtomicBool,
    runner: Arc<dyn LifecycleRunner>,
}

pub(super) trait LifecycleRunner: Send + Sync {
    fn run(
        &self,
        command: &ExactCliCommand,
        settings: &AppSettings,
        preflight: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
        commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
    ) -> RunEvidence;
    fn version(
        &self,
        context: &LifecycleContext,
        executable: &Path,
        runtime: Option<&NpmRuntime>,
        environment: &[(OsString, OsString)],
    ) -> Result<String, AgentLifecycleError>;
}

pub(super) struct ProcessRunner;

impl LifecycleRunner for ProcessRunner {
    fn run(
        &self,
        command: &ExactCliCommand,
        settings: &AppSettings,
        preflight: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
        commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>,
    ) -> RunEvidence {
        let (kind, output_truncated) =
            command.run_with_timeout(settings, UPGRADE_TIMEOUT, preflight, commit);
        RunEvidence {
            kind,
            output_truncated,
        }
    }

    fn version(
        &self,
        context: &LifecycleContext,
        executable: &Path,
        runtime: Option<&NpmRuntime>,
        environment: &[(OsString, OsString)],
    ) -> Result<String, AgentLifecycleError> {
        npm::probe_version(context, executable, runtime, environment)
    }
}

impl NativeExecution {
    pub(super) fn prepare(
        context: LifecycleContext,
        target: &AgentLifecycleTarget,
        mechanism: Mechanism,
        version: &str,
    ) -> Result<Self, AgentLifecycleError> {
        Self::prepare_with_runner(context, target, mechanism, version, Arc::new(ProcessRunner))
    }

    pub(super) fn prepare_with_runner(
        context: LifecycleContext,
        target: &AgentLifecycleTarget,
        mechanism: Mechanism,
        version: &str,
        runner: Arc<dyn LifecycleRunner>,
    ) -> Result<Self, AgentLifecycleError> {
        let directory =
            GuardedDirectory::capture_path(mechanism.directory()).map_err(map_mutation_error)?;
        if !owned_writable_directory(mechanism.directory()) {
            return Err(unavailable(
                AgentLifecycleUnavailableReason::PermissionRequired,
            ));
        }
        let selected_executable =
            ExecutableStamp::capture(&target.installation).map_err(map_mutation_error)?;
        let mut related_directories = Vec::new();
        let (command, npm_script) = match &mechanism {
            Mechanism::Homebrew(installation) => (installation.upgrade_command(&context)?, None),
            Mechanism::Npm {
                prefix, runtime, ..
            } => {
                related_directories.push(
                    GuardedDirectory::capture_path(&prefix.join(".balancehub-npm-cache"))
                        .map_err(map_mutation_error)?,
                );
                let (command, script) =
                    npm::upgrade_command(&context, target.agent_kind, prefix, runtime, version)?;
                (command, Some(script))
            }
            Mechanism::Vendor { launcher, .. } => {
                let mut overrides = npm::environment(&context.home);
                let argv = match target.agent_kind {
                    AgentCliKind::ClaudeCode => vec!["update".to_owned()],
                    AgentCliKind::Grok => {
                        overrides.push((
                            "GROK_HOME".into(),
                            context.home.join(".grok").into_os_string(),
                        ));
                        vec!["update".to_owned()]
                    }
                    _ => {
                        return Err(unavailable(
                            AgentLifecycleUnavailableReason::UnsupportedChannel,
                        ))
                    }
                };
                (
                    ExactCliCommand::from_path(launcher, argv, &context.home, overrides)
                        .map_err(map_mutation_error)?,
                    None,
                )
            }
        };
        Ok(Self {
            context,
            kind: target.agent_kind,
            mechanism,
            directory,
            related_directories,
            selected_executable,
            command: command.capture_output(),
            npm_script,
            previous_version: target
                .installation
                .installed_version
                .as_deref()
                .and_then(npm::normalized_version),
            upgrade_attempted: AtomicBool::new(false),
            runner,
        })
    }

    pub(super) fn command_preview(&self) -> Vec<String> {
        std::iter::once(
            self.command
                .executable_path()
                .to_string_lossy()
                .into_owned(),
        )
        .chain(self.command.argv.iter().cloned())
        .collect()
    }

    fn revalidate_sources(&self) -> Result<(), AgentLifecycleError> {
        self.directory.revalidate().map_err(map_mutation_error)?;
        for directory in &self.related_directories {
            directory.revalidate().map_err(map_mutation_error)?;
        }
        self.selected_executable
            .revalidate()
            .map_err(map_mutation_error)?;
        if let Some(script) = &self.npm_script {
            script.revalidate().map_err(map_mutation_error)?;
        }
        match &self.mechanism {
            Mechanism::Homebrew(installation) => installation.revalidate(&self.context)?,
            Mechanism::Npm {
                runtime,
                package,
                prefix,
            } => {
                runtime.manifest.revalidate()?;
                package.revalidate()?;
                match fs::symlink_metadata(prefix.join(".balancehub-empty.npmrc")) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return Err(changed()),
                }
            }
            Mechanism::Vendor {
                receipts,
                absent_receipts,
                ..
            } => {
                for receipt in receipts {
                    receipt.revalidate()?;
                }
                for path in absent_receipts {
                    match fs::symlink_metadata(path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        _ => return Err(changed()),
                    }
                }
            }
        }
        self.command.revalidate().map_err(map_mutation_error)
    }
}

impl LifecycleExecution for NativeExecution {
    fn diagnostics(&self) -> Option<AgentLifecycleDiagnostics> {
        self.command
            .output_snapshot()
            .map(|output| AgentLifecycleDiagnostics {
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.exit_code,
                error: output.error,
                truncated: output.truncated,
            })
    }

    fn revalidate(&self) -> Result<(), AgentLifecycleError> {
        self.revalidate_sources()
    }

    fn run(&self, commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>) -> RunEvidence {
        self.runner.run(
            &self.command,
            &self.context.settings,
            &mut || {
                self.revalidate_sources().map_err(|error| {
                    let mut failure =
                        AgentAssetMutationError::new(AgentAssetMutationErrorKind::SourceConflict);
                    failure.message = error.message;
                    failure
                })
            },
            &mut || {
                commit()?;
                self.upgrade_attempted.store(true, Ordering::Release);
                Ok(())
            },
        )
    }

    fn verify(&self) -> Verification {
        let (path, launcher, runtime, package_version) = match &self.mechanism {
            Mechanism::Homebrew(installation) => {
                let path = installation.verified_launcher();
                (path.clone(), path, None, None)
            }
            Mechanism::Npm {
                prefix, runtime, ..
            } => match npm::installed_package(prefix, self.kind) {
                Ok((package, _)) => {
                    let launcher = npm::verified_package_launcher(prefix, self.kind, &package);
                    (
                        Some(package.bin_path),
                        launcher,
                        Some(runtime.as_ref()),
                        Some(package.version),
                    )
                }
                Err(_) => (None, None, Some(runtime.as_ref()), None),
            },
            Mechanism::Vendor {
                launcher,
                directory,
                ..
            } => {
                let path = fs::canonicalize(launcher)
                    .ok()
                    .filter(|path| path.starts_with(directory));
                let path = path.map(|_| launcher.clone());
                (path.clone(), path, None, None)
            }
        };
        let environment = npm::environment(&self.context.home);
        let version = path
            .as_deref()
            .and_then(|path| {
                if matches!(self.mechanism, Mechanism::Homebrew(_)) {
                    crate::services::agent_cli::discovery::find_cli_at_path(
                        path,
                        crate::services::agent_cli::definition(self.kind),
                    )
                    .ok()
                    .and_then(|executable| npm::normalized_version(&executable.version))
                } else {
                    self.runner
                        .version(&self.context, path, runtime, &environment)
                        .ok()
                }
            })
            .filter(|version| {
                package_version
                    .as_ref()
                    .is_none_or(|package| package == version)
            });
        let original_changed = self.selected_executable.revalidate().is_err()
            || match &self.mechanism {
                Mechanism::Homebrew(installation) => installation.changed(),
                Mechanism::Npm { package, .. } => package.revalidate().is_err(),
                Mechanism::Vendor { receipts, .. } => {
                    receipts.iter().any(|stamp| stamp.revalidate().is_err())
                }
            };
        let changed = original_changed || (version.is_some() && version != self.previous_version);
        // An upgrade may touch transitive package files while leaving its
        // entrypoint/version unchanged. Once attempted, those two observations
        // cannot prove the entire prior installation unchanged.
        let unchanged = !self.upgrade_attempted.load(Ordering::Acquire)
            && !changed
            && self.previous_version.is_some()
            && version == self.previous_version
            && self.selected_executable.revalidate().is_ok();
        Verification {
            executable_path: version
                .as_ref()
                .and(launcher)
                .map(|path| path.to_string_lossy().into_owned()),
            version,
            changed,
            unchanged,
        }
    }
}
