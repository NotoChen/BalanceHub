//! Artifact provenance. Candidate paths or the configured/automatic source do
//! not establish a distribution. Require a matching package bin declaration or
//! a verified native vendor signature, and otherwise report Unknown.

use super::{probe::ProbedCli, AgentCliDefinition};
use crate::{
    models::AgentCliDistribution,
    services::agent_cli::environment::{parse_semantic_version, run::AgentInventoryRun},
};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_PACKAGE_BYTES: usize = 64 * 1024;
const MAX_PACKAGE_ANCESTORS: usize = 8;

#[derive(Debug, Clone)]
pub(in crate::services::agent_cli) struct NpmPackageOwner {
    pub(in crate::services::agent_cli) root: PathBuf,
    pub(in crate::services::agent_cli) manifest: PathBuf,
    pub(in crate::services::agent_cli) bin_path: PathBuf,
    pub(in crate::services::agent_cli) version: String,
}

impl NpmPackageOwner {
    /// Only npm's documented global layout establishes a writable prefix.
    /// pnpm stores, project node_modules and version-manager shims do not.
    pub(in crate::services::agent_cli) fn global_prefix(&self) -> Option<PathBuf> {
        let modules = if self.root.parent()?.file_name()?.to_str()?.starts_with('@') {
            self.root.parent()?.parent()?
        } else {
            self.root.parent()?
        };
        if modules.file_name()? != "node_modules" {
            return None;
        }
        #[cfg(windows)]
        let prefix = modules.parent()?;
        #[cfg(not(windows))]
        let prefix = {
            let lib = modules.parent()?;
            if lib.file_name()? != "lib" {
                return None;
            }
            lib.parent()?
        };
        // A Homebrew-owned libexec tree has a different update owner.
        if prefix.components().any(|part| {
            matches!(part, Component::Normal(value) if value == "Cellar" || value == "Caskroom")
        }) {
            return None;
        }
        Some(prefix.to_path_buf())
    }
}

pub(super) fn detect(
    cli: &ProbedCli,
    definition: &AgentCliDefinition,
    run: &AgentInventoryRun,
) -> AgentCliDistribution {
    let canonical = Path::new(&cli.canonical_path);
    if npm_package_owns_binary(
        canonical,
        definition.environment().package_name(),
        &cli.executable.version,
    ) {
        return AgentCliDistribution::Npm;
    }
    if verified_vendor_signature(cli, definition.environment().macos_vendor_signature(), run) {
        return AgentCliDistribution::VendorNative;
    }
    AgentCliDistribution::Unknown
}

fn npm_package_owns_binary(executable: &Path, package_name: &str, version: &str) -> bool {
    npm_package_owner(executable, package_name, version).is_some()
}

pub(in crate::services::agent_cli) fn npm_package_owner(
    executable: &Path,
    package_name: &str,
    version: &str,
) -> Option<NpmPackageOwner> {
    let version = parse_semantic_version(version)?;
    executable
        .ancestors()
        .skip(1)
        .take(MAX_PACKAGE_ANCESTORS)
        .find_map(|directory| {
            let manifest = directory.join("package.json");
            let Ok(metadata) = fs::symlink_metadata(&manifest) else {
                return None;
            };
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > MAX_PACKAGE_BYTES as u64
            {
                return None;
            }
            let Ok(file) = File::open(&manifest) else {
                return None;
            };
            let mut bytes = Vec::new();
            if file
                .take((MAX_PACKAGE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() > MAX_PACKAGE_BYTES
            {
                return None;
            }
            let Ok(package) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                return None;
            };
            if package.get("name").and_then(|value| value.as_str()) != Some(package_name)
                || package
                    .get("version")
                    .and_then(|value| value.as_str())
                    .and_then(parse_semantic_version)
                    .as_ref()
                    != Some(&version)
            {
                return None;
            }
            let matches_bin = |bin: &str| {
                let path = Path::new(bin);
                !path.as_os_str().is_empty()
                    && path.components().all(|component| {
                        matches!(component, Component::Normal(_) | Component::CurDir)
                    })
                    && fs::canonicalize(directory.join(path)).is_ok_and(|path| path == executable)
            };
            let bin = match package.get("bin") {
                Some(serde_json::Value::String(bin)) => matches_bin(bin).then_some(bin.as_str()),
                Some(serde_json::Value::Object(bins)) => bins
                    .values()
                    .filter_map(|value| value.as_str())
                    .find(|bin| matches_bin(bin)),
                _ => None,
            }?;
            Some(NpmPackageOwner {
                root: directory.to_path_buf(),
                manifest,
                bin_path: directory.join(bin),
                version: package.get("version")?.as_str()?.to_owned(),
            })
        })
}

#[cfg(target_os = "macos")]
fn verified_vendor_signature(
    cli: &ProbedCli,
    signature: Option<(&'static str, &'static str)>,
    run: &AgentInventoryRun,
) -> bool {
    use crate::platform::process::run_command_with_output_timeout;
    use std::{
        collections::BTreeMap,
        process::Command,
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    static VERIFIED: OnceLock<Mutex<BTreeMap<String, bool>>> = OnceLock::new();
    let Some((team, identifier)) = signature else {
        return false;
    };
    let key = format!("{}:{team}:{identifier}", cli.executable_revision);
    let cache = VERIFIED.get_or_init(Mutex::default);
    if let Some(result) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .copied()
    {
        return result;
    }
    let timeout = run
        .deadline()
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(2));
    if timeout.is_zero() {
        return false;
    }
    let deadline = Instant::now() + timeout;
    let Some(_permit) = super::probe::global_cli_process_gate().acquire_until(deadline) else {
        return false;
    };
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return false;
    }
    let mut command = Command::new("/usr/bin/codesign");
    command.args(["--verify", "--strict", "--requirement"])
        .arg(format!("=anchor apple generic and certificate leaf[subject.OU] = \"{team}\" and identifier \"{identifier}\""))
        .arg(&cli.canonical_path);
    let Ok(output) = run_command_with_output_timeout(
        &mut command,
        timeout,
        run.limits().cli_output_bytes.min(2048),
    ) else {
        return false;
    };
    // Transient timeout never becomes persistent negative provenance.
    if output.timed_out {
        return false;
    }
    let valid = output.status.is_some_and(|status| status.success());
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert(key, valid);
    valid
}

#[cfg(not(target_os = "macos"))]
fn verified_vendor_signature(
    _cli: &ProbedCli,
    _signature: Option<(&'static str, &'static str)>,
    _run: &AgentInventoryRun,
) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_requires_matching_package_version_and_exact_bin() {
        let mut nonce = [0; 8];
        getrandom::fill(&mut nonce).unwrap();
        let root = std::env::temp_dir().join(format!(
            "bh-package-provenance-{:x}",
            u64::from_ne_bytes(nonce)
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        let executable = root.join("bin/fixture.js");
        fs::write(&executable, "fixture").unwrap();
        let executable = fs::canonicalize(&executable).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"@fixture/agent","version":"1.2.3","bin":{"agent":"bin/fixture.js"}}"#,
        )
        .unwrap();
        assert!(npm_package_owns_binary(
            &executable,
            "@fixture/agent",
            "agent 1.2.3"
        ));
        assert!(!npm_package_owns_binary(
            &executable,
            "@fixture/other",
            "1.2.3"
        ));
        assert!(!npm_package_owns_binary(
            &executable,
            "@fixture/agent",
            "1.2.4"
        ));
        fs::write(
            root.join("package.json"),
            r#"{"name":"@fixture/agent","version":"1.2.3","bin":{"agent":"bin/other.js"}}"#,
        )
        .unwrap();
        assert!(!npm_package_owns_binary(
            &executable,
            "@fixture/agent",
            "1.2.3"
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
