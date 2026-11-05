use super::super::state::PreviousMatches;
use super::super::workspace_scan::scan_workspaces;
use super::{history_adapter, row_for_record, ReferenceState, SourceOutcome};
use crate::{
    models::{
        AgentSessionRow, AgentSessionSource, AgentSessionSourceState, AgentSessionSourceStatus,
        AgentSessionWorkspace,
    },
    services::{
        agent_cli::contracts::{SessionHistoryReadMode, SessionHistorySource, SessionReadBudget},
        cli_sessions::{transcript::Transcript, SearchAccumulator, SearchQuery},
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub(super) fn scan_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    workspaces: &[AgentSessionWorkspace],
    text: &str,
    budget: &SessionReadBudget,
    previous_matches: PreviousMatches,
) -> SourceOutcome {
    let mut outcome = SourceOutcome {
        references: Vec::new(),
        matched: HashSet::new(),
        states: Vec::new(),
        can_continue: false,
        workspaces: Vec::new(),
    };
    let failure = |message: String, status| {
        let ids = if workspaces.is_empty() {
            vec![None]
        } else {
            workspaces
                .iter()
                .map(|workspace| Some(workspace.id.clone()))
                .collect()
        };
        ids.into_iter()
            .map(|workspace_id| AgentSessionSourceState {
                source_id: source.id.clone(),
                workspace_id,
                state: status,
                loaded_count: 0,
                message: Some(message.clone()),
            })
            .collect::<Vec<_>>()
    };
    if !source.available {
        outcome.states = failure(
            "原生会话目录不存在或不可用".into(),
            AgentSessionSourceStatus::Unavailable,
        );
        return outcome;
    }
    if let crate::models::AgentRuntimeScope::Wsl { distro_id } = &source.runtime_scope {
        if let Err(error) = crate::services::wsl::require_running(distro_id) {
            outcome.states = failure(error, AgentSessionSourceStatus::Unavailable);
            return outcome;
        }
    }
    let adapter = match history_adapter(source.agent_kind) {
        Ok(adapter) => adapter,
        Err(error) => {
            outcome.states = failure(error, AgentSessionSourceStatus::Unsupported);
            return outcome;
        }
    };
    let query = match SearchQuery::new(text) {
        Ok(query) => query,
        Err(error) => {
            outcome.states = failure(error, AgentSessionSourceStatus::Partial);
            return outcome;
        }
    };
    let loaded = match scan_workspaces(
        adapter,
        source.agent_kind,
        &config.config_root,
        workspaces,
        budget,
        SessionHistoryReadMode::Summaries,
    ) {
        Ok(loaded) => loaded,
        Err(error) => {
            outcome.can_continue =
                budget.check().is_err() && !budget.cancelled.load(Ordering::Acquire);
            outcome.states = if outcome.can_continue {
                failure(
                    "正在继续读取其余会话".into(),
                    AgentSessionSourceStatus::Indexing,
                )
            } else {
                failure(error, AgentSessionSourceStatus::Partial)
            };
            return outcome;
        }
    };
    let full_request = SearchAccumulator::new(&query).content_request();
    let source_diagnostics = loaded.diagnostics;
    for loaded in loaded.workspaces {
        let workspace = workspaces
            .iter()
            .find(|workspace| std::path::Path::new(&workspace.path) == loaded.workdir)
            .cloned()
            .unwrap_or_else(|| super::super::scope::workspace_for_path(&loaded.workdir));
        outcome.workspaces.push(workspace.clone());
        let mut diagnostics = loaded
            .diagnostics
            .into_iter()
            .filter(|message| !message.contains("预算"))
            .collect::<Vec<_>>();
        if !workspaces.is_empty() {
            diagnostics.extend(source_diagnostics.iter().cloned());
        }
        let mut pending = false;
        let mut references = Vec::new();
        let mut incomplete = HashSet::new();
        let mut decisions = HashMap::new();
        for record in loaded.records {
            let row = row_for_record(source, config, &workspace, &record);
            let prior = previous_matches
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&row.session_ref)
                .cloned();
            if let Some(prior) = prior {
                decisions.insert(row.session_ref.clone(), prior);
            } else if metadata_match(&query, &row).complete() {
                decisions.insert(row.session_ref.clone(), (true, None));
            } else if record.content_unavailable_reason.is_none() {
                // Give the transcript read the remainder of this pass; a later
                // cursor resumes sessions that did not fit.
                if budget.deadline.saturating_duration_since(Instant::now())
                    < Duration::from_millis(250)
                {
                    pending = true;
                } else {
                    let mut matched = metadata_match(&query, &row);
                    match Transcript::load(
                        &workspace.id,
                        &record.record_key,
                        &record,
                        adapter,
                        budget,
                    ) {
                        Ok(transcript) => {
                            if transcript.skipped_records > 0 {
                                diagnostics.push(format!(
                                    "有 {} 条损坏或异常超长记录无法读取",
                                    transcript.skipped_records
                                ));
                            }
                            if transcript.truncated {
                                diagnostics
                                    .push("有会话正文过大，全文检索只覆盖最近加载的内容".into());
                            }
                            let result = transcript.search(&full_request);
                            matched.merge_content(result.clone());
                            if !transcript.complete && !transcript.truncated {
                                incomplete.insert(row.session_ref.clone());
                                pending = true;
                            }
                            // A partial transcript can still show what it has
                            // read; the incomplete marker keeps it out of the
                            // final-match cache below.
                            decisions
                                .insert(row.session_ref.clone(), (matched.complete(), result.hit));
                        }
                        Err(error) if budget.check().is_err() => {
                            let _ = error;
                            pending = true;
                        }
                        Err(error) => diagnostics.push(error),
                    }
                }
            } else {
                decisions.insert(row.session_ref.clone(), (false, None));
            }
            references.push(ReferenceState { row, record });
        }
        {
            let mut previous = previous_matches
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for reference in &mut references {
                if let Some((matched, hit)) = decisions.remove(&reference.row.session_ref) {
                    if matched {
                        outcome.matched.insert(reference.row.session_ref.clone());
                    }
                    reference.row.search_hit = hit.clone();
                    // Partial matches are useful immediately, but a later native
                    // rewind or update can change them. Cache final decisions only.
                    if reference.record.content_unavailable_reason.is_none()
                        && !incomplete.contains(&reference.row.session_ref)
                    {
                        previous.insert(reference.row.session_ref.clone(), (matched, hit));
                    }
                }
            }
        }
        let loaded_count = references.len();
        outcome.references.extend(references);
        pending |= budget.check().is_err();
        diagnostics.sort();
        diagnostics.dedup();
        let state = if budget.cancelled.load(Ordering::Acquire) {
            AgentSessionSourceStatus::Cancelled
        } else if pending {
            outcome.can_continue = true;
            diagnostics.insert(0, "正在按需检索其余会话".into());
            AgentSessionSourceStatus::Indexing
        } else if !loaded.complete || !diagnostics.is_empty() {
            AgentSessionSourceStatus::Partial
        } else {
            AgentSessionSourceStatus::Complete
        };
        outcome.states.push(AgentSessionSourceState {
            source_id: source.id.clone(),
            workspace_id: Some(workspace.id.clone()),
            state,
            loaded_count,
            message: (!diagnostics.is_empty()).then(|| diagnostics.join("；")),
        });
    }
    if workspaces.is_empty() {
        let cancelled = budget.cancelled.load(Ordering::Acquire);
        let pending = !cancelled && (outcome.can_continue || budget.check().is_err());
        outcome.can_continue |= pending;
        let state = if cancelled {
            AgentSessionSourceStatus::Cancelled
        } else if pending {
            AgentSessionSourceStatus::Indexing
        } else if !source_diagnostics.is_empty() {
            AgentSessionSourceStatus::Partial
        } else {
            AgentSessionSourceStatus::Complete
        };
        outcome.states.push(AgentSessionSourceState {
            source_id: source.id.clone(),
            workspace_id: None,
            state,
            loaded_count: outcome.references.len(),
            message: if pending {
                Some("正在按需读取其余会话".into())
            } else {
                (!source_diagnostics.is_empty()).then(|| source_diagnostics.join("；"))
            },
        });
    }
    outcome
}

fn metadata_match<'a>(query: &'a SearchQuery, row: &AgentSessionRow) -> SearchAccumulator<'a> {
    let mut matched = SearchAccumulator::new(query);
    matched.observe(&row.session.title);
    matched.observe(&row.session.id);
    matched.observe(&row.session.workdir);
    if let Some(preview) = &row.session.preview {
        matched.observe(preview);
    }
    if let Some(model) = &row.session.model {
        matched.observe(model);
    }
    for model in &row.session.models {
        matched.observe(model);
    }
    matched
}
