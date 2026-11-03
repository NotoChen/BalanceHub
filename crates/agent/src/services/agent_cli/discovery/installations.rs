use super::{
    candidates::{
        collect_all_candidates, explicit_env_values, CandidateCollectionMode, CandidateCollector,
        CliCandidate,
    },
    paths::RuntimePathSnapshot,
    probe::{self, ProbeFailure, ProbedCli},
    AgentCliDefinition,
};
use crate::models::{
    AgentAssetDiagnostic, AgentAssetLimitKind, AgentDiscoverySource, AgentExecutableIdentity,
    AgentInstallation, AgentInstallationAvailability, AgentVersionSource,
};
use crate::services::agent_cli::environment::run::AgentInventoryRun;
use std::{
    cmp::Ordering,
    env,
    sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
    time::Duration,
    time::Instant,
};

const CLI_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct CandidateProbeOutcome {
    candidate: CliCandidate,
    result: Result<ProbedCli, ProbeFailure>,
}

#[derive(Debug)]
struct SuccessfulCandidate {
    candidate: CliCandidate,
    probed: ProbedCli,
}

#[derive(Debug)]
struct OwnerGroup {
    owner_key: String,
    canonical_path: String,
    observations: Vec<SuccessfulCandidate>,
}

pub(in crate::services::agent_cli) fn discover_installations(
    preferred_path: &str,
    spec: &AgentCliDefinition,
    include_shell: bool,
    run: &mut AgentInventoryRun,
) -> Vec<AgentInstallation> {
    let candidate_limit = run.limits().candidate_paths_per_agent;
    let deadline = run.deadline();
    let preferred = super::paths::clean_preferred_path(preferred_path);
    let explicit = explicit_env_values(spec);
    let collection = collect_all_candidates(
        &preferred,
        &explicit,
        spec,
        include_shell,
        candidate_limit,
        CandidateCollectionMode::InventoryBounded { deadline },
    );
    let collector = collection.collector;
    if let Some(diagnostic) = candidate_truncation_diagnostic(&collector) {
        run.emit_inventory_diagnostic(diagnostic);
    }
    if collector.budget_exceeded {
        run.emit_inventory_diagnostic(AgentAssetDiagnostic::BudgetExceeded {
            elapsed_ms: run.limits().refresh_budget_ms,
            budget_ms: run.limits().refresh_budget_ms,
        });
        return Vec::new();
    }

    discover_installations_with_run(
        collector.candidates,
        spec,
        &collection.runtime_path,
        deadline,
        &|candidate, spec, runtime_path, deadline, timeout, output_limit| {
            probe::probe_cli_candidate_with_gate(
                &candidate.runtime_entrypoint,
                spec,
                runtime_path,
                deadline,
                timeout,
                output_limit,
                probe::global_cli_process_gate(),
            )
        },
        run,
    )
}

pub(super) fn discover_installations_with_run<F>(
    candidates: Vec<CliCandidate>,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
    deadline: Instant,
    probe_candidate: &F,
    run: &mut AgentInventoryRun,
) -> Vec<AgentInstallation>
where
    F: Fn(
            &CliCandidate,
            &AgentCliDefinition,
            &RuntimePathSnapshot,
            Instant,
            Duration,
            usize,
        ) -> Result<ProbedCli, ProbeFailure>
        + Sync,
{
    let cli_concurrency = run.limits().cli_concurrency;
    let cli_output_bytes = run.limits().cli_output_bytes;
    if cli_concurrency == 0 {
        run.emit_inventory_diagnostic(AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CliConcurrency,
            accepted: 0,
            observed_at_least: u64::from(!candidates.is_empty()),
        });
        return Vec::new();
    }
    if cli_output_bytes == 0 {
        run.emit_inventory_diagnostic(AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CliOutput,
            accepted: 0,
            observed_at_least: u64::from(!candidates.is_empty()),
        });
        return Vec::new();
    }

    let outcomes = probe_candidates(
        candidates,
        spec,
        runtime_path,
        cli_concurrency,
        cli_output_bytes,
        deadline,
        probe_candidate,
    );
    reduce_probe_outcomes(outcomes, spec, run)
}

fn probe_candidates<F>(
    candidates: Vec<CliCandidate>,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
    worker_limit: usize,
    output_limit: usize,
    deadline: Instant,
    probe_candidate: &F,
) -> Vec<CandidateProbeOutcome>
where
    F: Fn(
            &CliCandidate,
            &AgentCliDefinition,
            &RuntimePathSnapshot,
            Instant,
            Duration,
            usize,
        ) -> Result<ProbedCli, ProbeFailure>
        + Sync,
{
    let worker_count = worker_limit.min(candidates.len());
    if worker_count == 0 {
        return Vec::new();
    }

    let next_index = AtomicUsize::new(0);
    let mut outcomes = std::thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|_| {
                let candidates = &candidates;
                let next_index = &next_index;
                scope.spawn(move || {
                    let mut outcomes = Vec::new();
                    loop {
                        let index = next_index.fetch_add(1, AtomicOrdering::Relaxed);
                        let Some(candidate) = candidates.get(index).cloned() else {
                            break;
                        };
                        let result = if Instant::now() >= deadline {
                            Err(ProbeFailure::budget_exceeded())
                        } else {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                probe_candidate(
                                    &candidate,
                                    spec,
                                    runtime_path,
                                    deadline,
                                    CLI_VERSION_TIMEOUT,
                                    output_limit,
                                )
                            }))
                            .unwrap_or_else(|_| {
                                Err(ProbeFailure {
                                    kind: crate::models::AgentExecutableProbeErrorKind::Failed,
                                    message: "CLI 版本探测线程异常".to_string(),
                                    output_truncated: false,
                                    budget_exceeded: false,
                                })
                            })
                        };
                        outcomes.push(CandidateProbeOutcome { candidate, result });
                    }
                    outcomes
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .expect("candidate probe worker catches individual probe panics")
            })
            .collect::<Vec<_>>()
    });
    outcomes.sort_by_key(|outcome| outcome.candidate.ordinal);
    outcomes
}

fn reduce_probe_outcomes(
    outcomes: Vec<CandidateProbeOutcome>,
    spec: &AgentCliDefinition,
    run: &mut AgentInventoryRun,
) -> Vec<AgentInstallation> {
    let cli_output_bytes = run.limits().cli_output_bytes;
    let refresh_budget_ms = run.limits().refresh_budget_ms;
    let installations_per_agent = run.limits().installations_per_agent;
    let mut groups: Vec<OwnerGroup> = Vec::new();
    let mut budget_reported = false;
    for outcome in outcomes {
        match outcome.result {
            Ok(probed) => {
                let owner_key = probed.owner_key.clone();
                let canonical_path = probed.canonical_path.clone();
                let success = SuccessfulCandidate {
                    candidate: outcome.candidate,
                    probed,
                };
                if let Some(group) = groups.iter_mut().find(|group| {
                    group.owner_key == owner_key && group.canonical_path == canonical_path
                }) {
                    group.observations.push(success);
                } else {
                    groups.push(OwnerGroup {
                        owner_key,
                        canonical_path,
                        observations: vec![success],
                    });
                }
            }
            Err(failure) => {
                if failure.output_truncated {
                    run.emit_inventory_diagnostic(cli_output_truncated_diagnostic(
                        cli_output_bytes,
                    ));
                }
                if failure.budget_exceeded {
                    if !budget_reported {
                        run.emit_inventory_diagnostic(AgentAssetDiagnostic::BudgetExceeded {
                            elapsed_ms: refresh_budget_ms,
                            budget_ms: refresh_budget_ms,
                        });
                        budget_reported = true;
                    }
                } else if outcome.candidate.source == AgentDiscoverySource::Configured {
                    run.emit_inventory_diagnostic(AgentAssetDiagnostic::InstallationProbeFailed {
                        candidate_source: outcome.candidate.source,
                        error_kind: failure.kind,
                    });
                }
            }
        }
    }

    for group in &mut groups {
        group
            .observations
            .sort_by(|left, right| compare_candidate_priority(&left.candidate, &right.candidate));
    }
    groups.sort_by(|left, right| {
        compare_candidate_priority(
            &left.observations[0].candidate,
            &right.observations[0].candidate,
        )
    });

    if groups.len() > installations_per_agent {
        run.emit_inventory_diagnostic(AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::InstallationsPerAgent,
            accepted: installations_per_agent as u64,
            observed_at_least: installations_per_agent.saturating_add(1) as u64,
        });
        groups.truncate(installations_per_agent);
    }

    let environment_id = format!("native:{}", env::consts::OS);
    let installations = groups
        .into_iter()
        .map(|group| {
            let winner = &group.observations[0];
            let source = winner.candidate.source;
            let source_key = discovery_source_key(source);
            let id = super::super::environment::stable_id(
                "installation",
                &[
                    environment_id.as_str(),
                    spec.kind.key(),
                    source_key,
                    group.owner_key.as_str(),
                    group.canonical_path.as_str(),
                ],
            );
            let version = winner.probed.executable.version.clone();
            if group
                .observations
                .iter()
                .any(|observation| observation.probed.output_truncated)
            {
                run.emit_installation_diagnostic(
                    &id,
                    cli_output_truncated_diagnostic(cli_output_bytes),
                );
            }
            let distribution = super::distribution::detect(&winner.probed, spec, run);
            AgentInstallation {
                id,
                environment_id: environment_id.clone(),
                agent_kind: spec.kind,
                label: spec.label.to_string(),
                availability: AgentInstallationAvailability::Available,
                executable_path: Some(winner.probed.executable.path.clone()),
                executable_identity: Some(AgentExecutableIdentity {
                    owner: group.owner_key,
                    canonical_path: group.canonical_path,
                    installation_source: source,
                }),
                executable_revision: Some(winner.probed.executable_revision.clone()),
                installed_version: Some(version.clone()),
                discovery_source: source,
                distribution,
                channel: super::super::environment::version_channel(&version),
                installed_version_source: AgentVersionSource::LocalExecutable,
                diagnostics: Vec::new(),
            }
        })
        .collect();
    installations
}

pub(super) fn candidate_truncation_diagnostic(
    collector: &CandidateCollector,
) -> Option<AgentAssetDiagnostic> {
    collector
        .truncated
        .then(|| AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::CandidatePathsPerAgent,
            accepted: collector.candidates.len() as u64,
            observed_at_least: collector.candidates.len().saturating_add(1) as u64,
        })
}

fn cli_output_truncated_diagnostic(output_limit: usize) -> AgentAssetDiagnostic {
    AgentAssetDiagnostic::Truncated {
        limit: AgentAssetLimitKind::CliOutput,
        accepted: output_limit as u64,
        observed_at_least: output_limit.saturating_add(1) as u64,
    }
}

fn compare_candidate_priority(left: &CliCandidate, right: &CliCandidate) -> Ordering {
    discovery_source_rank(left.source)
        .cmp(&discovery_source_rank(right.source))
        .then_with(|| left.ordinal.cmp(&right.ordinal))
        .then_with(|| left.lexical_key.cmp(&right.lexical_key))
}

fn discovery_source_rank(source: AgentDiscoverySource) -> u8 {
    match source {
        AgentDiscoverySource::Configured => 0,
        AgentDiscoverySource::Automatic => 1,
    }
}

fn discovery_source_key(source: AgentDiscoverySource) -> &'static str {
    match source {
        AgentDiscoverySource::Configured => "configured",
        AgentDiscoverySource::Automatic => "automatic",
    }
}
