use super::{
    filesystem::{changed, FileStamp},
    planning::{unavailable, LifecycleContext},
    service::map_mutation_error,
};
use crate::{
    models::*,
    services::agent_cli::{
        self,
        discovery::{paths::runtime_binary_candidates, NpmPackageOwner},
        environment::{
            config_document::{self, ConfigDocumentFormat},
            mutation::execution::{ExactCliCommand, ExecutableStamp},
            parse_semantic_version,
        },
    },
};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const NPM_REGISTRY: &str = "https://registry.npmjs.org";

#[derive(Clone)]
pub(super) struct NpmRuntime {
    pub(super) node: PathBuf,
    pub(super) npm_cli: PathBuf,
    pub(super) node_version: String,
    pub(super) manifest: FileStamp,
}

impl NpmRuntime {
    pub(super) fn discover(
        context: &LifecycleContext,
        prefix: &Path,
    ) -> Result<Self, AgentLifecycleError> {
        let preferred = if cfg!(windows) {
            prefix.to_path_buf()
        } else {
            prefix.join("bin")
        };
        Self::discover_candidates(
            context,
            runtime_binary_candidates("node", Some(&preferred)),
            |command, timeout| {
                command
                    .probe_output(&context.settings, timeout)
                    .ok()
                    .as_ref()
                    .and_then(version_from_output)
            },
        )
    }

    pub(super) fn discover_candidates(
        context: &LifecycleContext,
        candidates: impl IntoIterator<Item = PathBuf>,
        mut probe: impl FnMut(&ExactCliCommand, Duration) -> Option<String>,
    ) -> Result<Self, AgentLifecycleError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        for node in candidates {
            if Instant::now() >= deadline {
                break;
            }
            if !matches!(executable_format(&node), Ok(ExecutableFormat::Native)) {
                continue;
            }
            let Some(parent) = node.parent() else {
                continue;
            };
            let npm_root = if cfg!(windows) {
                parent.join("node_modules/npm")
            } else {
                let Some(prefix) = parent.parent() else {
                    continue;
                };
                prefix.join("lib/node_modules/npm")
            };
            // Homebrew and version managers may expose npm through a symlink.
            // Pin the real package root, then retain exact file evidence there.
            let Ok(npm_root) = fs::canonicalize(npm_root) else {
                continue;
            };
            let npm_cli = npm_root.join("bin/npm-cli.js");
            let Ok((manifest, bytes)) = FileStamp::read(&npm_root.join("package.json"), 64 * 1024)
            else {
                continue;
            };
            let Some(package) = config_document::parse(&bytes, ConfigDocumentFormat::Json) else {
                continue;
            };
            if package.get("name").and_then(|value| value.as_str()) != Some("npm")
                || package.pointer("/bin/npm").and_then(|value| value.as_str())
                    != Some("bin/npm-cli.js")
                || !npm_cli.is_file()
            {
                continue;
            }
            let Ok(command) = ExactCliCommand::from_path(
                &node,
                vec!["--version".to_owned()],
                &context.home,
                environment(&context.home),
            ) else {
                continue;
            };
            if let Some(node_version) =
                probe(&command, deadline.saturating_duration_since(Instant::now()))
            {
                if semver::Version::parse(&node_version).is_err() {
                    continue;
                }
                return Ok(Self {
                    node,
                    npm_cli,
                    node_version,
                    manifest,
                });
            }
        }
        Err(unavailable(
            AgentLifecycleUnavailableReason::RuntimeUnavailable,
        ))
    }

    pub(super) fn signature(&self) -> String {
        format!(
            "{}\0{}\0{}\0{}",
            self.node.display(),
            self.npm_cli.display(),
            self.node_version,
            self.manifest.signature()
        )
    }
}

pub(super) fn package_root(prefix: &Path, kind: AgentCliKind) -> PathBuf {
    let modules = if cfg!(windows) {
        prefix.join("node_modules")
    } else {
        prefix.join("lib/node_modules")
    };
    modules.join(agent_cli::definition(kind).environment().package_name())
}

/// Reads only the exact registry package and its named executable. It does not
/// treat an arbitrary node_modules directory or a matching name as provenance.
pub(super) fn installed_package(
    prefix: &Path,
    kind: AgentCliKind,
) -> Result<(NpmPackageOwner, FileStamp), AgentLifecycleError> {
    let root = package_root(prefix, kind);
    let (stamp, bytes) = FileStamp::read(&root.join("package.json"), 64 * 1024)?;
    let document =
        config_document::parse(&bytes, ConfigDocumentFormat::Json).ok_or_else(changed)?;
    let definition = agent_cli::definition(kind);
    if document.get("name").and_then(|value| value.as_str())
        != Some(definition.environment().package_name())
    {
        return Err(changed());
    }
    let version = document
        .get("version")
        .and_then(|value| value.as_str())
        .ok_or_else(changed)?;
    if semver::Version::parse(version).is_err() {
        return Err(changed());
    }
    let bin = match document.get("bin") {
        Some(serde_json::Value::String(value)) => Some(value.as_str()),
        Some(serde_json::Value::Object(values)) => values
            .get(definition.executable)
            .and_then(|value| value.as_str()),
        _ => None,
    }
    .ok_or_else(changed)?;
    let relative = Path::new(bin);
    if relative.as_os_str().is_empty()
        || relative.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return Err(changed());
    }
    let bin_path = root.join(relative);
    let canonical = fs::canonicalize(&bin_path).map_err(|_| changed())?;
    let owner = agent_cli::discovery::npm_package_owner(
        &canonical,
        definition.environment().package_name(),
        version,
    )
    .ok_or_else(changed)?;
    if owner.global_prefix().as_deref() != Some(prefix) {
        return Err(changed());
    }
    Ok((owner, stamp))
}

pub(super) fn verified_package_launcher(
    prefix: &Path,
    kind: AgentCliKind,
    package: &NpmPackageOwner,
) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let launcher = prefix
            .join("bin")
            .join(agent_cli::definition(kind).executable);
        let actual = fs::canonicalize(&launcher).ok()?;
        let expected = fs::canonicalize(&package.bin_path).ok()?;
        (actual == expected).then_some(launcher)
    }
    #[cfg(not(unix))]
    {
        let _ = (prefix, kind);
        // Package scripts can be version-probed through the selected Node, but
        // that does not prove an npm-generated .cmd wrapper safe to adopt.
        matches!(
            executable_format(&package.bin_path),
            Ok(ExecutableFormat::Native)
        )
        .then(|| package.bin_path.clone())
    }
}

pub(super) fn upgrade_command(
    context: &LifecycleContext,
    kind: AgentCliKind,
    prefix: &Path,
    runtime: &NpmRuntime,
    version: &str,
) -> Result<(ExactCliCommand, ExecutableStamp), AgentLifecycleError> {
    if semver::Version::parse(version).is_err() {
        return Err(changed());
    }
    let script_stamp =
        ExecutableStamp::capture_path(&runtime.npm_cli).map_err(map_mutation_error)?;
    let mut argv = vec![
        runtime.npm_cli.to_string_lossy().into_owned(),
        "install".to_owned(),
        "--global".to_owned(),
        "--prefix".to_owned(),
        prefix.to_string_lossy().into_owned(),
        format!(
            "{}@latest",
            agent_cli::definition(kind).environment().package_name()
        ),
        "--registry".to_owned(),
        NPM_REGISTRY.to_owned(),
        "--userconfig".to_owned(),
        null_config_path().to_owned(),
        "--globalconfig".to_owned(),
        prefix
            .join(".balancehub-empty.npmrc")
            .to_string_lossy()
            .into_owned(),
        "--cache".to_owned(),
        prefix
            .join(".balancehub-npm-cache")
            .to_string_lossy()
            .into_owned(),
    ];
    argv.extend(
        [
            "--no-audit",
            "--no-fund",
            "--loglevel=notice",
            "--fetch-retries=0",
            "--update-notifier=false",
            "--engine-strict",
            "--yes",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    let command = ExactCliCommand::from_path(
        &runtime.node,
        argv,
        &context.home,
        environment(&context.home),
    )
    .map_err(map_mutation_error)?;
    Ok((command, script_stamp))
}

pub(super) fn probe_version(
    context: &LifecycleContext,
    executable: &Path,
    runtime: Option<&NpmRuntime>,
    environment: &[(OsString, OsString)],
) -> Result<String, AgentLifecycleError> {
    let (program, argv) = match executable_format(executable)? {
        ExecutableFormat::Node => {
            let runtime = runtime
                .ok_or_else(|| unavailable(AgentLifecycleUnavailableReason::RuntimeUnavailable))?;
            (
                runtime.node.as_path(),
                vec![
                    executable.to_string_lossy().into_owned(),
                    "--version".to_owned(),
                ],
            )
        }
        ExecutableFormat::Native => (executable, vec!["--version".to_owned()]),
    };
    let command = ExactCliCommand::from_path(program, argv, &context.home, environment.to_vec())
        .map_err(map_mutation_error)?;
    let output = command
        .probe_output(&context.settings, Duration::from_secs(5))
        .map_err(map_mutation_error)?;
    version_from_output(&output).ok_or_else(changed)
}

pub(super) fn environment(home: &Path) -> Vec<(OsString, OsString)> {
    vec![
        ("HOME".into(), home.as_os_str().to_owned()),
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
    ]
}

pub(super) fn normalized_version(raw: &str) -> Option<String> {
    let version = parse_semantic_version(raw)?;
    let mut text = format!("{}.{}.{}", version.major, version.minor, version.patch);
    if !version.prerelease.is_empty() {
        text.push('-');
        text.push_str(
            &version
                .prerelease
                .iter()
                .map(|part| match part {
                    AgentVersionIdentifier::Numeric(value) => value.to_string(),
                    AgentVersionIdentifier::Text(value) => value.clone(),
                })
                .collect::<Vec<_>>()
                .join("."),
        );
    }
    Some(text)
}

fn version_from_output(output: &crate::platform::process::CommandOutput) -> Option<String> {
    if output.timed_out
        || output.stdout_truncated
        || output.stderr_truncated
        || !output.status.is_some_and(|status| status.success())
    {
        return None;
    }
    output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .find_map(normalized_version)
}

#[derive(PartialEq, Eq)]
enum ExecutableFormat {
    Native,
    Node,
}

fn executable_format(path: &Path) -> Result<ExecutableFormat, AgentLifecycleError> {
    let mut file = fs::File::open(path).map_err(|_| changed())?;
    let mut bytes = [0_u8; 256];
    let count = file.read(&mut bytes).map_err(|_| changed())?;
    let bytes = &bytes[..count];
    if bytes.starts_with(b"#!")
        && bytes
            .split(|byte| *byte == b'\n')
            .next()
            .is_some_and(|line| line.windows(4).any(|part| part == b"node"))
    {
        return Ok(ExecutableFormat::Node);
    }
    if bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(b"MZ")
        || [
            b"\xcf\xfa\xed\xfe".as_slice(),
            b"\xfe\xed\xfa\xcf",
            b"\xca\xfe\xba\xbe",
            b"\xbe\xba\xfe\xca",
        ]
        .iter()
        .any(|magic| bytes.starts_with(magic))
    {
        return Ok(ExecutableFormat::Native);
    }
    Err(unavailable(
        AgentLifecycleUnavailableReason::ProvenanceUnverified,
    ))
}

fn null_config_path() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}
