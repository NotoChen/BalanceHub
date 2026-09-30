use super::{
    super::verified_path::{inspect_verified_path, reopen_verified_path, VerifiedPathAnchor},
    files::conflict,
};
use crate::{
    models::*,
    network::{apply_proxy_env, resolve_global_proxy},
    platform::process::{
        run_command_with_output_observed, run_command_with_output_timeout_before_spawn,
        CommandOutput, ProcessOutputCapture, ProcessOutputSnapshot,
    },
    services::agent_cli::discovery::paths::{runtime_path_for, runtime_path_without_shell_for},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

const CLI_TIMEOUT: Duration = Duration::from_secs(15);
const EXECUTABLE_HASH_BUDGET: Duration = Duration::from_secs(5);
const EXECUTABLE_MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct ExactCliCommand {
    pub(crate) argv: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) environment: Vec<(OsString, OsString)>,
    executable: ExecutableStamp,
    output_capture: Option<Arc<ProcessOutputCapture>>,
}

#[derive(Clone)]
pub(crate) struct ExecutableStamp {
    launcher: PathBuf,
    canonical: PathBuf,
    anchor: VerifiedPathAnchor,
    digest: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliApplyEvidence {
    ExitedSuccessfully,
    ExitedUnsuccessfully,
    TimedOut,
    SpawnFailed,
    RejectedBeforeSpawn,
}

impl ExactCliCommand {
    pub(crate) fn from_path(
        executable: &Path,
        argv: Vec<String>,
        cwd: &Path,
        environment: Vec<(OsString, OsString)>,
    ) -> Result<Self, AgentAssetMutationError> {
        Self::from_stamp(
            ExecutableStamp::capture_path(executable)?,
            argv,
            cwd,
            environment,
            false,
        )
    }

    pub(crate) fn new(
        installation: &AgentInstallation,
        argv: Vec<String>,
        cwd: &Path,
        environment: Vec<(OsString, OsString)>,
    ) -> Result<Self, AgentAssetMutationError> {
        Self::from_stamp(
            ExecutableStamp::capture(installation)?,
            argv,
            cwd,
            environment,
            true,
        )
    }

    fn from_stamp(
        executable: ExecutableStamp,
        argv: Vec<String>,
        cwd: &Path,
        environment: Vec<(OsString, OsString)>,
        include_shell: bool,
    ) -> Result<Self, AgentAssetMutationError> {
        if argv.is_empty() || argv.iter().any(|part| part.contains('\0')) || !cwd.is_absolute() {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::InvalidRequest,
            ));
        }
        let environment = launch_environment(&executable.launcher, environment, include_shell)?;
        Ok(Self {
            argv,
            cwd: cwd.to_path_buf(),
            environment,
            executable,
            output_capture: None,
        })
    }

    pub(crate) fn capture_output(mut self) -> Self {
        self.output_capture = Some(Arc::new(ProcessOutputCapture::default()));
        self
    }

    pub(crate) fn output_snapshot(&self) -> Option<ProcessOutputSnapshot> {
        self.output_capture
            .as_ref()
            .map(|capture| capture.snapshot())
    }

    pub(crate) fn signature(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.executable.digest);
        digest.update(self.executable.canonical.as_os_str().as_encoded_bytes());
        for arg in &self.argv {
            digest.update([0]);
            digest.update(arg.as_bytes());
        }
        digest.update(self.cwd.as_os_str().as_encoded_bytes());
        for (key, value) in &self.environment {
            digest.update([0]);
            digest.update(key.as_encoded_bytes());
            digest.update([0]);
            digest.update(value.as_encoded_bytes());
        }
        format!("{:x}", digest.finalize())
    }

    pub(crate) fn revalidate(&self) -> Result<(), AgentAssetMutationError> {
        self.executable.revalidate()
    }

    pub(crate) fn executable_path(&self) -> &Path {
        &self.executable.canonical
    }

    pub(super) fn run(
        &self,
        settings: &AppSettings,
        preflight: impl FnOnce() -> Result<(), AgentAssetMutationError>,
        before_spawn: impl FnOnce() -> Result<(), AgentAssetMutationError>,
    ) -> (CliApplyEvidence, bool) {
        self.run_with_timeout(settings, CLI_TIMEOUT, preflight, before_spawn)
    }

    pub(crate) fn run_with_timeout(
        &self,
        settings: &AppSettings,
        timeout: Duration,
        preflight: impl FnOnce() -> Result<(), AgentAssetMutationError>,
        before_spawn: impl FnOnce() -> Result<(), AgentAssetMutationError>,
    ) -> (CliApplyEvidence, bool) {
        let mut command = self.command(settings);
        let timeout = timeout.min(Duration::from_secs(15 * 60));
        let mut allowed_to_spawn = false;
        let result = run_command_with_output_observed(
            &mut command,
            timeout,
            if self.output_capture.is_some() {
                16 * 1024
            } else {
                AgentAssetLimits::DEFAULT.cli_output_bytes / 2
            },
            || {
                self.revalidate()
                    .and_then(|_| preflight())
                    .map_err(|error| {
                        std::io::Error::new(std::io::ErrorKind::Interrupted, error.message)
                    })?;
                Ok(())
            },
            || {
                before_spawn().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::Interrupted, error.message)
                })?;
                allowed_to_spawn = true;
                Ok(())
            },
            self.output_capture.clone(),
        );
        match result {
            // Asset commands discard output; lifecycle commands explicitly opt in.
            Ok(output) => {
                if let Some(capture) = &self.output_capture {
                    capture.finish(
                        output.status.and_then(|status| status.code()),
                        output
                            .timed_out
                            .then(|| "安装命令超时，已请求终止进程".to_owned()),
                    );
                }
                let truncated = output.stdout_truncated || output.stderr_truncated;
                let evidence = if output.timed_out || output.status.is_none() {
                    CliApplyEvidence::TimedOut
                } else if output.status.is_some_and(|status| status.success()) {
                    CliApplyEvidence::ExitedSuccessfully
                } else {
                    CliApplyEvidence::ExitedUnsuccessfully
                };
                (evidence, truncated)
            }
            Err(error) => {
                if let Some(capture) = &self.output_capture {
                    capture.finish(None, Some(error.to_string()));
                }
                (
                    if allowed_to_spawn {
                        CliApplyEvidence::SpawnFailed
                    } else {
                        CliApplyEvidence::RejectedBeforeSpawn
                    },
                    false,
                )
            }
        }
    }

    /// Read-only lifecycle probes reuse the same exact executable, environment,
    /// proxy resolution and managed process/output budget as confirmed actions.
    pub(crate) fn probe_output(
        &self,
        settings: &AppSettings,
        timeout: Duration,
    ) -> Result<CommandOutput, AgentAssetMutationError> {
        let mut command = self.command(settings);
        run_command_with_output_timeout_before_spawn(
            &mut command,
            timeout.min(CLI_TIMEOUT),
            AgentAssetLimits::DEFAULT.cli_output_bytes / 2,
            || {
                self.revalidate()
                    .map_err(|_| std::io::ErrorKind::Interrupted.into())
            },
            || Ok(()),
        )
        .map_err(|_| conflict())
    }

    fn command(&self, settings: &AppSettings) -> Command {
        let mut command = Command::new(&self.executable.canonical);
        command
            .env_clear()
            .args(&self.argv)
            .current_dir(&self.cwd)
            .envs(self.environment.iter().cloned());
        apply_proxy_env(&mut command, &resolve_global_proxy(settings));
        command
    }
}

fn launch_environment(
    launcher: &Path,
    overrides: Vec<(OsString, OsString)>,
    include_shell: bool,
) -> Result<Vec<(OsString, OsString)>, AgentAssetMutationError> {
    let mut environment = BTreeMap::new();
    for key in ["SystemRoot", "WINDIR", "COMSPEC", "TMPDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            environment.insert(OsString::from(key), value);
        }
    }
    if let Some(path) = if include_shell {
        runtime_path_for(launcher)
    } else {
        runtime_path_without_shell_for(launcher)
    } {
        environment.insert(OsString::from("PATH"), path);
    }
    for (key, value) in [
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("TERM", "dumb"),
        ("NO_COLOR", "1"),
        ("CI", "1"),
    ] {
        environment.insert(OsString::from(key), OsString::from(value));
    }
    environment.extend(overrides);
    let home = environment
        .get(std::ffi::OsStr::new("HOME"))
        .filter(|home| Path::new(home).is_absolute())
        .ok_or_else(|| AgentAssetMutationError::new(AgentAssetMutationErrorKind::InvalidRequest))?
        .clone();
    #[cfg(windows)]
    environment.insert(OsString::from("USERPROFILE"), home);
    #[cfg(not(windows))]
    let _ = home;
    // NODE_OPTIONS, credential variables and native config overrides from the
    // GUI process must not redirect a command away from its planned context.
    // Network proxy variables are added solely by the existing network module.
    Ok(environment.into_iter().collect())
}

impl ExecutableStamp {
    pub(crate) fn capture_path(path: &Path) -> Result<Self, AgentAssetMutationError> {
        let canonical = std::fs::canonicalize(path).map_err(|_| conflict())?;
        Self::capture_at(path.to_path_buf(), canonical, None)
    }

    pub(crate) fn capture(
        installation: &AgentInstallation,
    ) -> Result<Self, AgentAssetMutationError> {
        let identity = installation
            .executable_identity
            .as_ref()
            .ok_or_else(conflict)?;
        let launcher = installation
            .executable_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(conflict)?;
        let canonical = PathBuf::from(&identity.canonical_path);
        let revision = installation
            .executable_revision
            .as_deref()
            .ok_or_else(conflict)?;
        Self::capture_at(launcher, canonical, Some(revision))
    }

    fn capture_at(
        launcher: PathBuf,
        canonical: PathBuf,
        revision: Option<&str>,
    ) -> Result<Self, AgentAssetMutationError> {
        if !canonical.is_absolute()
            || std::fs::canonicalize(&launcher).ok().as_ref() != Some(&canonical)
        {
            return Err(conflict());
        }
        let root = canonical.parent().ok_or_else(conflict)?;
        let guard = inspect_verified_path(&[root], root, &canonical, AgentAssetSourceKind::File)
            .map_err(|_| conflict())?;
        let digest = digest_executable(guard.source_handle())?;
        guard.revalidate().map_err(|_| conflict())?;
        let revision = AgentAssetRevision {
            identity: revision
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{:x}", Sha256::digest(digest))),
            observed_at: String::new(),
            size_bytes: Some(guard.metadata().map_err(|_| conflict())?.len()),
            is_missing: false,
            is_directory: false,
            is_symlink: false,
        };
        Ok(Self {
            launcher,
            canonical,
            anchor: guard.anchor(&revision, None),
            digest,
        })
    }

    pub(crate) fn revalidate(&self) -> Result<(), AgentAssetMutationError> {
        if std::fs::canonicalize(&self.launcher).ok().as_ref() != Some(&self.canonical) {
            return Err(conflict());
        }
        let guard = reopen_verified_path(&self.anchor).map_err(|_| conflict())?;
        if digest_executable(guard.source_handle())? != self.digest {
            return Err(conflict());
        }
        guard.revalidate().map_err(|_| conflict())
    }
}

fn digest_executable(file: &File) -> Result<[u8; 32], AgentAssetMutationError> {
    let metadata = file.metadata().map_err(|_| conflict())?;
    if metadata.len() > EXECUTABLE_MAX_BYTES || !metadata.is_file() {
        return Err(conflict());
    }
    let deadline = Instant::now() + EXECUTABLE_HASH_BUDGET;
    let mut reader = file.try_clone().map_err(|_| conflict())?;
    reader.seek(SeekFrom::Start(0)).map_err(|_| conflict())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut read = 0_u64;
    loop {
        if Instant::now() >= deadline {
            return Err(conflict());
        }
        let count = reader.read(&mut buffer).map_err(|_| conflict())?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > EXECUTABLE_MAX_BYTES {
            return Err(conflict());
        }
        digest.update(&buffer[..count]);
    }
    if read != metadata.len() {
        return Err(conflict());
    }
    Ok(digest.finalize().into())
}
