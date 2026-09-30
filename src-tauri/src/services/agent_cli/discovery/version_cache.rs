use super::{
    paths::RuntimePathSnapshot,
    probe::{CliProcessGate, ProbeFailure, ProbedCli},
    AgentCliDefinition,
};
use crate::services::agent_cli::cache;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static ENTRIES: OnceLock<Mutex<BTreeMap<String, Arc<VersionEntry>>>> = OnceLock::new();

#[derive(Default)]
struct VersionEntry {
    value: Mutex<Option<ProbedCli>>,
    gate: OnceLock<CliProcessGate>,
}

pub(in crate::services::agent_cli) fn initialize(root: PathBuf) {
    let _ = ROOT.set(root);
}

pub(super) fn cached(
    candidate: &Path,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
    deadline: Instant,
    probe: impl FnOnce() -> Result<ProbedCli, ProbeFailure>,
) -> Result<ProbedCli, ProbeFailure> {
    let mut digest = Sha256::new();
    digest.update(b"agent-cli-version-v1");
    digest.update(spec.kind.key());
    cache::observe_environment(&mut digest, spec.kind);
    cache::observe_path(&mut digest, candidate);
    let canonical = std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf());
    cache::observe_path(&mut digest, &canonical);
    // npm wrappers can stay unchanged while their package/native binary changes.
    for parent in canonical.ancestors().skip(1).take(6) {
        let manifest = parent.join("package.json");
        cache::observe_path(&mut digest, &manifest);
        if manifest.is_file() {
            cache::observe_path(&mut digest, parent);
            cache::observe_path(&mut digest, &parent.join("node_modules"));
            break;
        }
    }
    if let Some(path) = runtime_path.path_for(candidate) {
        digest.update(path.as_encoded_bytes());
        for directory in std::env::split_paths(&path) {
            cache::observe_path(
                &mut digest,
                &directory.join(if cfg!(windows) { "node.exe" } else { "node" }),
            );
        }
    }
    let key = format!("{:x}", digest.finalize());
    let entry = ENTRIES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .entry(key.clone())
        .or_default()
        .clone();
    let _permit = entry
        .gate
        .get_or_init(|| CliProcessGate::new(1))
        .acquire_until(deadline)
        .ok_or_else(ProbeFailure::budget_exceeded)?;
    let mut value = entry
        .value
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let path = ROOT.get().map(|root| root.join(format!("{key}.json")));
    if value.is_none() {
        *value = path.as_deref().and_then(cache::read);
    }
    if let Some(value) = value.as_ref() {
        return Ok(value.clone());
    }
    let result = probe()?;
    // Failed or truncated probes are retried; they never become persistent facts.
    if !result.output_truncated {
        if let Some(path) = path {
            let _ = cache::write(&path, &result);
        }
        *value = Some(result.clone());
    }
    Ok(result)
}
