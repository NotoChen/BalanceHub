use super::{history_adapter, path_identity, row_for_record, ReferenceState, SourceOutcome};
use crate::{
    models::{
        AgentSessionSource, AgentSessionSourceState, AgentSessionSourceStatus,
        AgentSessionWorkspace, CliSessionIndexState,
    },
    services::{
        agent_cli::contracts::{SessionHistoryReadMode, SessionHistorySource, SessionReadBudget},
        cli_sessions::{index::HistoryIndex, SearchAccumulator, SearchQuery},
    },
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};

pub(super) fn scan_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    workspaces: &[AgentSessionWorkspace],
    text: &str,
    index: &HistoryIndex,
    budget: &SessionReadBudget,
    previous_matches: Arc<Mutex<HashMap<String, bool>>>,
) -> SourceOutcome {
    let mut outcome = SourceOutcome {
        references: Vec::new(),
        matched: HashSet::new(),
        states: Vec::new(),
        can_continue: false,
    };
    let index_state = index.state();
    let failure = |message: String, status| {
        workspaces
            .iter()
            .map(|workspace| AgentSessionSourceState {
                source_id: source.id.clone(),
                workspace_id: workspace.id.clone(),
                state: status,
                loaded_count: 0,
                message: Some(message.clone()),
                index_state,
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
    let paths = workspaces
        .iter()
        .map(|workspace| PathBuf::from(&workspace.path))
        .collect::<Vec<_>>();
    let loaded = match super::super::budget::with_source_root(&config.config_root, || {
        (adapter.scan)(
            source.agent_kind,
            &config.config_root,
            &paths,
            budget,
            SessionHistoryReadMode::Summaries,
        )
    }) {
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
    for loaded in loaded {
        let Some(workspace) = workspaces.iter().find(|workspace| {
            path_identity(std::path::Path::new(&workspace.path)) == path_identity(&loaded.workdir)
        }) else {
            continue;
        };
        let mut diagnostics = loaded
            .diagnostics
            .into_iter()
            .filter(|message| !message.contains("预算"))
            .collect::<Vec<_>>();
        let mut pending = false;
        let mut indexed_bytes = 0u64;
        let mut source_bytes = 0u64;
        let mut state = AgentSessionSourceState {
            source_id: source.id.clone(),
            workspace_id: workspace.id.clone(),
            state: AgentSessionSourceStatus::Complete,
            loaded_count: loaded.records.len(),
            message: None,
            index_state,
        };
        // Native duplicate indexes are merged by the adapter's project/record
        // identity, never by a UI title or a bare cross-source session ID.
        for record in crate::services::cli_sessions::normalize_records(loaded.records) {
            let row = row_for_record(source, config, workspace, &record);
            let ref_key = row.session_ref.clone();
            let prior = previous_matches
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&ref_key)
                .copied();
            let matches: Result<Option<bool>, String> =
                prior.map(|value| Ok(Some(value))).unwrap_or_else(|| {
                    let mut matched = SearchAccumulator::new(&query);
                    matched.observe(&record.summary.title);
                    matched.observe(&record.summary.id);
                    if let Some(preview) = &record.summary.preview {
                        matched.observe(preview);
                    }
                    if let Some(model) = &record.summary.model {
                        matched.observe(model);
                    }
                    for model in &record.summary.models {
                        matched.observe(model);
                    }
                    matched.observe(&workspace.path);
                    if !matched.complete() && record.content_unavailable_reason.is_none() {
                        budget.check()?;
                        let request = matched.content_request();
                        match index.search(&workspace.id, &record, adapter, &request, budget) {
                            Ok(result) => {
                                state.index_state = index.state();
                                if result.skipped_records > 0 {
                                    diagnostics.push(format!(
                                        "会话 {} 有 {} 条异常超长记录未计入搜索",
                                        record.summary.id, result.skipped_records
                                    ));
                                }
                                if !result.complete {
                                    pending = true;
                                    indexed_bytes =
                                        indexed_bytes.saturating_add(result.indexed_bytes);
                                    source_bytes = source_bytes.saturating_add(result.source_bytes);
                                    return Ok(None);
                                }
                                matched.merge_content(result.content);
                            }
                            Err(error) => {
                                budget.check()?;
                                state.index_state = CliSessionIndexState::Fallback;
                                diagnostics
                                    .push(format!("索引暂不可用，已改用原生正文扫描：{error}"));
                                matched.merge_content((adapter.search)(
                                    &record,
                                    &request,
                                    &|| budget.check().is_ok(),
                                )?);
                            }
                        }
                    }
                    Ok(Some(matched.complete()))
                });
            match matches {
                Ok(Some(value)) => {
                    if value || record.content_unavailable_reason.is_none() {
                        previous_matches
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .insert(ref_key.clone(), value);
                    }
                    if value {
                        outcome.matched.insert(ref_key);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    if budget.check().is_err() {
                        pending = true;
                    } else {
                        diagnostics.push(error);
                    }
                }
            }
            outcome.references.push(ReferenceState { row, record });
        }
        pending |= budget.check().is_err();
        diagnostics.sort();
        diagnostics.dedup();
        if budget.cancelled.load(Ordering::Acquire) {
            state.state = AgentSessionSourceStatus::Cancelled;
        } else if pending {
            outcome.can_continue = true;
            state.state = if diagnostics.is_empty() {
                AgentSessionSourceStatus::Indexing
            } else {
                AgentSessionSourceStatus::Partial
            };
            diagnostics.insert(
                0,
                if source_bytes > 0 {
                    format!(
                        "正在检索会话正文，已读取 {:.1}%，将自动继续",
                        (indexed_bytes as f64 / source_bytes as f64 * 100.0).min(100.0)
                    )
                } else {
                    "正在继续读取其余会话".into()
                },
            );
        } else if !loaded.complete || !diagnostics.is_empty() {
            state.state = AgentSessionSourceStatus::Partial;
        }
        state.message = (!diagnostics.is_empty()).then(|| diagnostics.join("；"));
        outcome.states.push(state);
    }
    if !query.is_empty() && !outcome.can_continue {
        if let Err(error) = index.finish(budget) {
            // Cache maintenance does not make a successfully read source
            // incomplete, and must not create an endless continuation cursor.
            for state in &mut outcome.states {
                let message = format!("搜索已完成，索引缓存整理稍后继续：{error}");
                if let Some(previous) = &mut state.message {
                    previous.push('；');
                    previous.push_str(&message);
                } else {
                    state.message = Some(message);
                }
            }
        }
    }
    outcome
}
