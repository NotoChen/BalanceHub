use super::super::state::PreviousMatches;
use super::super::workspace_scan::scan_workspaces;
use super::{history_adapter, row_for_record, ReferenceState, SourceOutcome};
use crate::{
    models::{
        AgentSessionRow, AgentSessionSource, AgentSessionSourceState, AgentSessionSourceStatus,
        AgentSessionWorkspace,
    },
    services::{
        agent_cli::contracts::{
            SessionContentSearchRequest, SessionHistoryReadMode, SessionHistorySource,
            SessionReadBudget,
        },
        cli_sessions::{index::HistoryIndex, SearchAccumulator, SearchQuery},
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
    index: &HistoryIndex,
    budget: &SessionReadBudget,
    previous_matches: PreviousMatches,
) -> SourceOutcome {
    let mut outcome = SourceOutcome {
        references: Vec::new(),
        matched: HashSet::new(),
        states: Vec::new(),
        can_continue: false,
    };
    let failure = |message: String, status| {
        workspaces
            .iter()
            .map(|workspace| AgentSessionSourceState {
                source_id: source.id.clone(),
                workspace_id: workspace.id.clone(),
                state: status,
                loaded_count: 0,
                message: Some(message.clone()),
                index_state: index.state(),
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
    let empty_request = SessionContentSearchRequest { terms: Vec::new() };
    for loaded in loaded {
        let Some(workspace) = workspaces
            .iter()
            .find(|workspace| std::path::Path::new(&workspace.path) == loaded.workdir)
        else {
            continue;
        };
        let mut diagnostics = loaded
            .diagnostics
            .into_iter()
            .filter(|message| !message.contains("预算"))
            .collect::<Vec<_>>();
        let mut pending = false;
        let mut references = Vec::new();
        let mut indexed = HashSet::new();
        let mut incomplete = HashSet::new();
        let mut decisions = HashMap::new();
        for record in loaded.records {
            let row = row_for_record(source, config, workspace, &record);
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
                // Leave time for the one FTS query that delivers this batch.
                if budget.deadline.saturating_duration_since(Instant::now())
                    < Duration::from_millis(250)
                {
                    pending = true;
                } else {
                    match index.search(&workspace.id, &record, adapter, &empty_request, budget) {
                        Ok(progress) => {
                            indexed.insert(record.record_key.clone());
                            if progress.skipped_records > 0 {
                                diagnostics.push(format!(
                                    "有 {} 条损坏或异常超长记录无法读取",
                                    progress.skipped_records
                                ));
                            }
                            if !progress.complete {
                                incomplete.insert(record.record_key.clone());
                                pending = true;
                            }
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
        // The query runs once per workspace, including tools and thought text.
        // Native scans only update changed sources; no per-session FTS roundtrip.
        if !indexed.is_empty() {
            match index.search_workspace(&workspace.id, &full_request, budget) {
                Ok(mut results) => {
                    for reference in &references {
                        if !indexed.contains(&reference.record.record_key) {
                            continue;
                        }
                        let result = results
                            .remove(&reference.record.record_key)
                            .unwrap_or_default();
                        let hit = result.hit.clone();
                        let mut matched = metadata_match(&query, &reference.row);
                        matched.merge_content(result);
                        decisions
                            .insert(reference.row.session_ref.clone(), (matched.complete(), hit));
                    }
                }
                Err(error) if budget.check().is_err() => {
                    let _ = error;
                    pending = true;
                }
                Err(error) => diagnostics.push(error),
            }
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
                        && !incomplete.contains(&reference.record.record_key)
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
            diagnostics.insert(0, "正在增量检索其余会话".into());
            AgentSessionSourceStatus::Indexing
        } else if !loaded.complete || !diagnostics.is_empty() {
            AgentSessionSourceStatus::Partial
        } else {
            AgentSessionSourceStatus::Complete
        };
        outcome.states.push(AgentSessionSourceState {
            source_id: source.id.clone(),
            workspace_id: workspace.id.clone(),
            state,
            loaded_count,
            message: (!diagnostics.is_empty()).then(|| diagnostics.join("；")),
            index_state: index.state(),
        });
    }
    if !query.is_empty() && !outcome.can_continue {
        if let Err(error) = index.finish(budget) {
            for state in &mut outcome.states {
                let maintenance = format!("索引缓存整理稍后继续：{error}");
                state.message = Some(match state.message.take() {
                    Some(message) => format!("{message}；{maintenance}"),
                    None => maintenance,
                });
            }
        }
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
