use super::paths::{
    binary_names, capture_shell_discovery, expand_home_path, has_path_separator, home_dir,
    lexical_comparison_key, platform_global_dirs, process_path_candidates,
    AgentHomeCandidateScanRequest, AgentHomeScanCompletion, RuntimePathSnapshot,
    ShellDiscoverySnapshot,
};
use super::AgentCliDefinition;
use crate::models::AgentDiscoverySource;
use std::{cmp::Ordering, collections::BTreeSet, env, path::PathBuf, time::Instant};

#[derive(Debug, Clone)]
pub(super) struct CliCandidate {
    pub runtime_entrypoint: PathBuf,
    pub lexical_key: String,
    pub source: AgentDiscoverySource,
    pub fixed_priority: bool,
    pub ordinal: usize,
}

pub(super) struct CandidateCollector {
    pub candidates: Vec<CliCandidate>,
    limit: usize,
    pub truncated: bool,
    pub budget_exceeded: bool,
}

impl CandidateCollector {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            candidates: Vec::new(),
            limit,
            truncated: false,
            budget_exceeded: false,
        }
    }

    pub(super) fn push(
        &mut self,
        runtime_entrypoint: PathBuf,
        source: AgentDiscoverySource,
        fixed_priority: bool,
    ) {
        let lexical_key = lexical_comparison_key(&runtime_entrypoint);
        if let Some(existing) = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.lexical_key == lexical_key)
        {
            if source == AgentDiscoverySource::Configured {
                existing.source = AgentDiscoverySource::Configured;
            }
            existing.fixed_priority |= fixed_priority;
            return;
        }
        if self.candidates.len() >= self.limit {
            self.truncated = true;
            return;
        }
        let ordinal = self.candidates.len();
        self.candidates.push(CliCandidate {
            runtime_entrypoint,
            lexical_key,
            source,
            fixed_priority,
            ordinal,
        });
    }

    fn extend(
        &mut self,
        candidates: impl IntoIterator<Item = PathBuf>,
        source: AgentDiscoverySource,
        fixed_priority: bool,
    ) {
        // Search locations are possibilities, not installations. Explicitly
        // configured paths use push directly, retaining reportable failures.
        for candidate in candidates
            .into_iter()
            .filter(|candidate| candidate.is_file())
        {
            self.push(candidate, source, fixed_priority);
            if self.truncated {
                break;
            }
        }
    }

    fn lexical_keys(&self) -> BTreeSet<String> {
        self.candidates
            .iter()
            .map(|candidate| candidate.lexical_key.clone())
            .collect()
    }

    fn remaining_with_one_over(&self) -> usize {
        self.limit
            .saturating_sub(self.candidates.len())
            .saturating_add(1)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CandidateCollectionMode {
    Legacy,
    InventoryBounded { deadline: Instant },
}

pub(super) struct CandidateCollectionResult {
    pub collector: CandidateCollector,
    pub runtime_path: RuntimePathSnapshot,
}

pub(super) fn collect_all_candidates(
    preferred_path: &str,
    explicit_env_values: &[String],
    spec: &AgentCliDefinition,
    include_shell: bool,
    limit: usize,
    mode: CandidateCollectionMode,
) -> CandidateCollectionResult {
    let preferred_can_move = preferred_path_is_version_managed(preferred_path);
    let mut collector = CandidateCollector::new(limit);
    let shell = if include_shell {
        capture_shell_discovery(&shell_queries(preferred_path, explicit_env_values, spec))
    } else {
        ShellDiscoverySnapshot::default()
    };
    let mut runtime_home_dirs = Vec::new();

    if !preferred_path.is_empty() {
        collector.push(
            expand_home_path(preferred_path),
            AgentDiscoverySource::Configured,
            !preferred_can_move,
        );
        if !has_path_separator(preferred_path) && !collector.truncated {
            collector.extend(
                process_path_candidates(preferred_path),
                AgentDiscoverySource::Configured,
                !preferred_can_move,
            );
            if include_shell && !collector.truncated {
                collector.extend(
                    shell.candidates_for(preferred_path),
                    AgentDiscoverySource::Configured,
                    !preferred_can_move,
                );
            }
        }
    }

    for value in explicit_env_values {
        if collector.truncated {
            break;
        }
        collector.push(
            expand_home_path(value),
            AgentDiscoverySource::Configured,
            true,
        );
        if !has_path_separator(value) && !collector.truncated {
            collector.extend(
                process_path_candidates(value),
                AgentDiscoverySource::Configured,
                true,
            );
            if include_shell && !collector.truncated {
                collector.extend(
                    shell.candidates_for(value),
                    AgentDiscoverySource::Configured,
                    true,
                );
            }
        }
    }

    if !collector.truncated {
        if let Some(home) = home_dir() {
            let request = match mode {
                CandidateCollectionMode::Legacy => AgentHomeCandidateScanRequest::legacy(&home),
                CandidateCollectionMode::InventoryBounded { deadline } => {
                    AgentHomeCandidateScanRequest::inventory(
                        &home,
                        deadline,
                        collector.remaining_with_one_over(),
                        collector.lexical_keys(),
                    )
                }
            };
            let home_scan = (spec.home_scan)(request);
            runtime_home_dirs = home_scan.runtime_dirs;
            collector.extend(home_scan.candidates, AgentDiscoverySource::Automatic, false);
            match home_scan.completion {
                AgentHomeScanCompletion::Complete => {}
                AgentHomeScanCompletion::Truncated => collector.truncated = true,
                AgentHomeScanCompletion::BudgetExceeded => collector.budget_exceeded = true,
            }
        }
    }

    if !collector.truncated && !collector.budget_exceeded {
        for dir in platform_global_dirs() {
            collector.extend(
                binary_names(spec.executable)
                    .into_iter()
                    .map(|name| PathBuf::from(dir).join(name)),
                AgentDiscoverySource::Automatic,
                false,
            );
            if collector.truncated {
                break;
            }
        }
    }

    if !collector.truncated && !collector.budget_exceeded {
        collector.extend(
            process_path_candidates(spec.executable),
            AgentDiscoverySource::Automatic,
            false,
        );
    }
    if include_shell && !collector.truncated && !collector.budget_exceeded {
        collector.extend(
            shell.candidates_for(spec.executable),
            AgentDiscoverySource::Automatic,
            false,
        );
    }

    CandidateCollectionResult {
        collector,
        runtime_path: RuntimePathSnapshot::from_parts(runtime_home_dirs, shell.path()),
    }
}

fn shell_queries(
    preferred_path: &str,
    explicit_env_values: &[String],
    spec: &AgentCliDefinition,
) -> Vec<String> {
    std::iter::once(preferred_path)
        .chain(explicit_env_values.iter().map(String::as_str))
        .chain(std::iter::once(spec.executable))
        .filter(|value| !value.is_empty() && !has_path_separator(value))
        .map(str::to_string)
        .collect()
}

pub(super) fn explicit_env_values(spec: &AgentCliDefinition) -> Vec<String> {
    let mut values = Vec::new();
    let balancehub_key = balancehub_cli_path_env_key(spec.kind);
    for key in
        std::iter::once(balancehub_key.as_str()).chain(spec.additional_env_keys.iter().copied())
    {
        if let Ok(value) = env::var(key) {
            let value = super::paths::clean_preferred_path(&value);
            if !value.is_empty() {
                values.push(value);
            }
        }
    }
    values
}

pub(super) fn balancehub_cli_path_env_key(kind: crate::models::AgentCliKind) -> String {
    let mut key = String::from("BALANCEHUB_");
    let mut previous_was_lowercase = false;
    for character in kind.key().chars() {
        if character.is_ascii_uppercase() && previous_was_lowercase {
            key.push('_');
        }
        key.push(character.to_ascii_uppercase());
        previous_was_lowercase = character.is_ascii_lowercase();
    }
    key.push_str("_CLI_PATH");
    key
}

pub(super) fn candidate_matches_preferred(candidate: &CliCandidate, preferred_path: &str) -> bool {
    !preferred_path.is_empty()
        && candidate.lexical_key == lexical_comparison_key(&expand_home_path(preferred_path))
}

pub(super) fn preferred_path_is_version_managed(preferred_path: &str) -> bool {
    if preferred_path.is_empty() {
        return false;
    }
    let path = expand_home_path(preferred_path)
        .to_string_lossy()
        .replace('\\', "/");
    path.contains("/.nvm/versions/node/")
        || path.contains("/.fnm/node-versions/")
        || path.contains("/.local/share/fnm/node-versions/")
        || path.contains("/.local/state/fnm_multishells/")
}

pub(super) fn numeric_version_key(value: &str) -> Vec<u64> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

pub(super) fn compare_version_keys(left: &[u64], right: &[u64]) -> Ordering {
    let length = left.len().max(right.len());
    for index in 0..length {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    Ordering::Equal
}
