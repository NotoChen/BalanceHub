use crate::limits;

mod candidates;
mod distribution;
mod installations;
pub(super) mod paths;
mod probe;
#[cfg(test)]
mod tests;
mod version_cache;

use super::{AgentCliDefinition, AgentCliExecutable};
use crate::models::AgentDiscoverySource;
use candidates::{
    candidate_matches_preferred, collect_all_candidates, compare_version_keys, explicit_env_values,
    numeric_version_key, preferred_path_is_version_managed, CandidateCollectionMode,
    CandidateCollector,
};
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    path::Path,
    time::{Duration, Instant},
};

pub(super) use distribution::{npm_package_owner, NpmPackageOwner};
pub(super) use installations::discover_installations;
pub(super) use paths::runtime_path_for;
use paths::{clean_preferred_path, expand_home_path, RuntimePathSnapshot};
pub(super) use version_cache::initialize as initialize_cache;

#[cfg(test)]
use candidates::{balancehub_cli_path_env_key, CliCandidate};
#[cfg(test)]
use installations::{candidate_truncation_diagnostic, discover_installations_with_run};
#[cfg(test)]
use paths::{has_path_separator, lexical_comparison_key};
#[cfg(test)]
use probe::{ProbeFailure, ProbedCli};

const CLI_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// Shallow discovery and filesystem metadata only; never runs a CLI or login shell.
pub(super) fn observe_candidates(
    digest: &mut sha2::Sha256,
    preferred: &str,
    spec: &AgentCliDefinition,
) {
    use sha2::Digest;
    let collection = collect_all_candidates(
        preferred,
        &explicit_env_values(spec),
        spec,
        false,
        crate::models::AgentAssetLimits::HARD_CAP.candidate_paths_per_agent,
        CandidateCollectionMode::InventoryBounded {
            deadline: Instant::now() + Duration::from_secs(1),
        },
    );
    digest.update([
        u8::from(collection.collector.truncated),
        u8::from(collection.collector.budget_exceeded),
    ]);
    for candidate in collection.collector.candidates {
        super::cache::observe_path(digest, &candidate.runtime_entrypoint);
        if let Ok(canonical) = std::fs::canonicalize(&candidate.runtime_entrypoint) {
            super::cache::observe_path(digest, &canonical);
            for parent in canonical.ancestors().skip(1).take(6) {
                let manifest = parent.join("package.json");
                super::cache::observe_path(digest, &manifest);
                if manifest.is_file() {
                    super::cache::observe_path(digest, parent);
                    break;
                }
            }
        }
        if let Some(path) = collection
            .runtime_path
            .path_for(&candidate.runtime_entrypoint)
        {
            digest.update(path.as_encoded_bytes());
        }
    }
}

/// 显式自定义路径有效时优先使用；NVM/FNM 版本路径与自动发现候选则选择最高版本。
pub(super) fn find_cli(
    preferred_path: &str,
    spec: &AgentCliDefinition,
    include_shell: bool,
) -> Result<AgentCliExecutable, String> {
    let preferred_path = clean_preferred_path(preferred_path);
    let preferred_can_move = preferred_path_is_version_managed(&preferred_path);
    let explicit_values = explicit_env_values(spec);
    let mut fixed = CandidateCollector::new(usize::MAX);
    if !preferred_path.is_empty() && !preferred_can_move {
        fixed.push(
            expand_home_path(&preferred_path),
            AgentDiscoverySource::Configured,
            true,
        );
    }
    for value in &explicit_values {
        fixed.push(
            expand_home_path(value),
            AgentDiscoverySource::Configured,
            true,
        );
    }

    // Keep the fixed-path fast path genuinely staged. A configured launcher
    // normally finds its interpreter beside itself or on the process PATH, so
    // it must not wait for login-shell or version-manager discovery first.
    let fast_runtime_path = RuntimePathSnapshot::from_parts(Vec::new(), None);
    for candidate in &fixed.candidates {
        if let Ok(result) =
            probe_candidate_for_legacy(&candidate.runtime_entrypoint, spec, &fast_runtime_path)
        {
            return Ok(result);
        }
    }

    // If the fast path failed, build one complete legacy snapshot and retry
    // those candidates through the common collector. This preserves launchers
    // whose interpreter exists only in a version-manager or login-shell PATH.
    let collection = collect_all_candidates(
        &preferred_path,
        &explicit_values,
        spec,
        include_shell,
        usize::MAX,
        CandidateCollectionMode::Legacy,
    );
    let runtime_path = collection.runtime_path;

    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    let mut best: Option<(AgentCliExecutable, Vec<u64>)> = None;
    for candidate in collection.collector.candidates {
        if !seen.insert(candidate.lexical_key.clone()) {
            continue;
        }
        match probe_candidate_for_legacy(&candidate.runtime_entrypoint, spec, &runtime_path) {
            Ok(result) => {
                if candidate.fixed_priority {
                    return Ok(result);
                }
                let version_key = numeric_version_key(&result.version);
                let should_replace = best.as_ref().is_none_or(|(_, current_key)| {
                    compare_version_keys(&version_key, current_key) == Ordering::Greater
                });
                if should_replace {
                    best = Some((result, version_key));
                }
            }
            Err(message) => {
                if failures.len() < 4
                    || candidate_matches_preferred(&candidate, &preferred_path)
                    || candidate.source == AgentDiscoverySource::Configured
                {
                    failures.push(format!(
                        "{}: {message}",
                        candidate.runtime_entrypoint.display()
                    ));
                }
            }
        }
    }

    if let Some((result, _)) = best {
        return Ok(result);
    }

    failures.truncate(4);
    let not_found_message = format!(
        "未自动检测到可用的 {}{}",
        spec.label,
        if spec.label.to_ascii_lowercase().ends_with("cli") {
            ""
        } else {
            " CLI"
        }
    );
    if failures.is_empty() {
        Err(not_found_message)
    } else {
        Err(format!("{not_found_message}；{}", failures.join("；")))
    }
}

pub(super) fn current_installation_id(
    preferred_path: &str,
    spec: &AgentCliDefinition,
    installations: &[crate::models::AgentInstallation],
) -> Option<String> {
    use crate::models::AgentInstallationAvailability;
    let candidates: Vec<_> = installations
        .iter()
        .filter(|installation| {
            installation.agent_kind == spec.kind
                && installation.availability == AgentInstallationAvailability::Available
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let selected = find_cli(preferred_path, spec, false).ok()?;
    let selected_path = Path::new(&selected.path);
    let exact = candidates.iter().find(|installation| {
        installation.executable_path.as_deref().map(Path::new) == Some(selected_path)
    });
    if let Some(installation) = exact {
        return Some(installation.id.clone());
    }
    let canonical = std::fs::canonicalize(selected_path).ok()?;
    candidates
        .iter()
        .find(|installation| {
            installation
                .executable_identity
                .as_ref()
                .is_some_and(|identity| Path::new(&identity.canonical_path) == canonical)
        })
        .map(|installation| installation.id.clone())
}

pub(super) fn find_cli_at_path(
    candidate: &Path,
    spec: &AgentCliDefinition,
) -> Result<AgentCliExecutable, String> {
    let runtime_path = paths::legacy_runtime_path_snapshot(false);
    probe_candidate_for_legacy(candidate, spec, &runtime_path)
}

fn probe_candidate_for_legacy(
    candidate: &Path,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
) -> Result<AgentCliExecutable, String> {
    let deadline = Instant::now() + CLI_VERSION_TIMEOUT;
    probe::probe_cli_candidate(
        candidate,
        spec,
        runtime_path,
        deadline,
        CLI_VERSION_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .map(|probed| probed.executable)
    .map_err(|failure| failure.message)
}
