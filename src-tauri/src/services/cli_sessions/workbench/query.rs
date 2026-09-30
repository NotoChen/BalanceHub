use super::{
    budget::with_history_cache,
    hash,
    scope::{history_adapter, path_identity, validated_scope},
    state::{self, QueryProgress, ReferenceState, ScopeState, Snapshot},
    with_read_budget, AgentSessionActor,
};
use crate::{
    models::*,
    services::{
        agent_cli::contracts::{
            SessionHistoryReadMode, SessionHistoryRecord, SessionHistorySource, SessionReadBudget,
        },
        cli_sessions::{
            index::{HistoryIndex, SessionIndexConfig},
            session_sort_key, SearchAccumulator, SearchQuery,
        },
    },
    state::AppState,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::Manager;

struct SourceOutcome {
    references: Vec<ReferenceState>,
    matched: HashSet<String>,
    states: Vec<AgentSessionSourceState>,
    can_continue: bool,
}

pub(crate) fn query(
    actor: AgentSessionActor<'_>,
    request: &AgentSessionQuery,
    budget: &SessionReadBudget,
) -> Result<AgentSessionPage, String> {
    if !(1..=200).contains(&request.page_size) {
        return Err("每页会话数量应为 1 至 200".into());
    }
    SearchQuery::new(&request.query)?;
    let scope = validated_scope(actor, &request.scope_revision)?;
    let selected_workspaces = select_workspaces(&scope, &request.workspace_ids)?;
    let selected_sources = scope
        .public
        .sources
        .iter()
        .filter(|source| {
            request.agent_kinds.is_empty() || request.agent_kinds.contains(&source.agent_kind)
        })
        .cloned()
        .collect::<Vec<_>>();
    let key = query_key(request, &selected_workspaces, &selected_sources);
    let (snapshot_id, offset, mut snapshot) = if let Some(cursor) = request.cursor.as_deref() {
        let (id, offset) = cursor
            .rsplit_once(':')
            .ok_or_else(|| "会话分页引用无效".to_string())?;
        let offset = offset
            .parse::<usize>()
            .map_err(|_| "会话分页位置无效".to_string())?;
        let snapshot = state::registry()
            .snapshots
            .get(id)
            .cloned()
            .ok_or_else(|| "会话分页快照已过期，请刷新".to_string())?;
        if snapshot.actor != actor.key()
            || snapshot.consumer != request.consumer_id
            || snapshot.scope_revision != request.scope_revision
            || snapshot.query_key != key
            || offset > snapshot.rows.len()
        {
            return Err("会话分页不属于当前筛选范围，请刷新".into());
        }
        (id.to_string(), offset, snapshot)
    } else {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = hash(&[
            "snapshot",
            &actor.key(),
            &key,
            &SEQUENCE.fetch_add(1, Ordering::Relaxed).to_string(),
        ]);
        let snapshot = Snapshot {
            actor: actor.key(),
            consumer: request.consumer_id.clone(),
            scope_revision: request.scope_revision.clone(),
            query_key: key,
            rows: Vec::new(),
            parent_updates: HashMap::new(),
            states: Vec::new(),
            complete: false,
            progress: QueryProgress::default(),
            created_at: Instant::now(),
        };
        (id, 0, snapshot)
    };
    if request.cursor.is_none() || (offset == snapshot.rows.len() && snapshot.progress.can_continue)
    {
        let settings = actor
            .app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .settings
            .clone();
        let index_config = super::super::index_config(actor.app, &settings)?;
        snapshot.progress.passes += 1;
        let outcomes = std::thread::scope(|threads| {
            let mut handles = Vec::new();
            for source in selected_sources.iter().cloned() {
                let source_config = scope
                    .sources
                    .get(&source.id)
                    .cloned()
                    .ok_or_else(|| "原生来源已不可用".to_string())?;
                let cache = snapshot
                    .progress
                    .source_caches
                    .entry(source.id.clone())
                    .or_default()
                    .clone();
                let workspaces = selected_workspaces.clone();
                let index_config = index_config.clone();
                let query_text = request.query.clone();
                let matches = snapshot.progress.matches.clone();
                // Fast sources have independent workers and budgets. Later
                // cursors reuse completed records and advance unread sources.
                let duration =
                    Duration::from_secs((8 * u64::from(snapshot.progress.passes)).min(45));
                let source_budget = SessionReadBudget::new(
                    budget.deadline.min(Instant::now() + duration),
                    budget.cancelled.clone(),
                    budget.max_bytes,
                );
                handles.push(threads.spawn(move || {
                    with_read_budget(&source_budget, || {
                        with_history_cache(&cache, || {
                            super::budget::with_source_root(&source_config.config_root, || {
                                scan_source(
                                    &source,
                                    &source_config,
                                    &workspaces,
                                    &query_text,
                                    &index_config,
                                    &source_budget,
                                    matches,
                                )
                            })
                        })
                    })
                }));
            }
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| "原生会话读取线程异常".to_string())
                })
                .collect::<Result<Vec<_>, String>>()
        })?;
        if budget.cancelled.load(Ordering::Acquire) {
            return Err("会话读取已取消".into());
        }
        validated_scope(actor, &request.scope_revision)?;
        let refs = merge_source_outcomes(&mut snapshot, outcomes, request.role_filter);
        if snapshot.progress.passes > 1 {
            for source in &mut snapshot.states {
                source.message = Some(match source.message.take() {
                    Some(message) => {
                        format!("{message}；本次按续读批次保持稳定顺序，刷新可重新按时间排序")
                    }
                    None => "本次按续读批次保持稳定顺序，刷新可重新按时间排序".into(),
                });
            }
        }
        let mut registry = state::registry();
        for reference in refs {
            registry.references.insert(
                (
                    actor.key(),
                    request.scope_revision.clone(),
                    reference.row.session_ref.clone(),
                ),
                reference,
            );
        }
        registry.prune();
        registry
            .snapshots
            .insert(snapshot_id.clone(), snapshot.clone());
    }
    budget.check()?;
    let mut page = page(
        &snapshot_id,
        &snapshot,
        offset,
        request.page_size,
        &selected_sources,
        &selected_workspaces,
    );
    apply_activity(&mut page.items);
    Ok(page)
}

fn select_workspaces(
    scope: &ScopeState,
    ids: &[String],
) -> Result<Vec<AgentSessionWorkspace>, String> {
    if ids.is_empty() {
        return Ok(scope
            .public
            .workspaces
            .iter()
            .filter(|workspace| workspace.is_home || scope.public.workspaces.len() == 1)
            .cloned()
            .collect());
    }
    if ids.iter().any(|id| {
        !scope
            .public
            .workspaces
            .iter()
            .any(|workspace| &workspace.id == id)
    }) {
        return Err("所选工作目录已移出当前范围".into());
    }
    Ok(scope
        .public
        .workspaces
        .iter()
        .filter(|workspace| ids.contains(&workspace.id))
        .cloned()
        .collect())
}
fn query_key(
    request: &AgentSessionQuery,
    workspaces: &[AgentSessionWorkspace],
    sources: &[AgentSessionSource],
) -> String {
    let mut ids = workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .chain(sources.iter().map(|source| source.id.clone()))
        .collect::<Vec<_>>();
    ids.sort();
    hash(&[
        &ids.join("\n"),
        &format!("{:?}", request.role_filter),
        request.query.trim(),
    ])
}
fn role_matches(filter: AgentSessionRoleFilter, role: AgentSessionRole) -> bool {
    match filter {
        AgentSessionRoleFilter::All => true,
        AgentSessionRoleFilter::Main => role == AgentSessionRole::Main,
        AgentSessionRoleFilter::Subagent => role == AgentSessionRole::Subagent,
    }
}

fn merge_source_outcomes(
    snapshot: &mut Snapshot,
    outcomes: Vec<SourceOutcome>,
    role_filter: AgentSessionRoleFilter,
) -> Vec<ReferenceState> {
    let mut references = Vec::new();
    let mut matched_ids = HashSet::new();
    snapshot.states.clear();
    snapshot.progress.can_continue = false;
    for outcome in outcomes {
        references.extend(outcome.references);
        matched_ids.extend(outcome.matched);
        snapshot.states.extend(outcome.states);
        snapshot.progress.can_continue |= outcome.can_continue;
    }
    // Resolve against all actual native records before search, role filtering
    // and paging. A parent may itself be a subagent or outside the result page.
    link_parent_references(&mut references);
    let parents: HashMap<_, _> = references
        .iter()
        .map(|entry| (entry.row.session_ref.as_str(), &entry.row.parent))
        .collect();
    for row in &mut snapshot.rows {
        if let Some(parent) = parents.get(row.session_ref.as_str()) {
            if row.parent != **parent {
                row.parent = (*parent).clone();
                snapshot
                    .parent_updates
                    .insert(row.session_ref.clone(), row.parent.clone());
            }
        }
    }
    let new_rows = references
        .iter()
        .filter(|entry| {
            matched_ids.contains(&entry.row.session_ref)
                && role_matches(role_filter, entry.row.role)
        })
        .map(|entry| entry.row.clone())
        .collect();
    append_batch(&mut snapshot.rows, new_rows);
    snapshot.complete = snapshot
        .states
        .iter()
        .all(|state| state.state == AgentSessionSourceStatus::Complete);
    references
}

fn link_parent_references(references: &mut [ReferenceState]) {
    let mut parents = HashMap::<_, Option<String>>::new();
    for entry in references.iter() {
        let key = (
            entry.row.source_id.clone(),
            entry.row.workspace_id.clone(),
            entry.row.session.id.clone(),
        );
        parents
            .entry(key)
            .and_modify(|parent| {
                if parent.as_deref() != Some(entry.row.session_ref.as_str()) {
                    *parent = None;
                }
            })
            .or_insert_with(|| Some(entry.row.session_ref.clone()));
    }
    for entry in references {
        if let AgentSessionParent::Known {
            native_id,
            parent_ref,
        } = &mut entry.row.parent
        {
            *parent_ref = parents
                .get(&(
                    entry.row.source_id.clone(),
                    entry.row.workspace_id.clone(),
                    native_id.clone(),
                ))
                .and_then(Clone::clone);
        }
    }
}

fn append_batch(existing: &mut Vec<AgentSessionRow>, mut incoming: Vec<AgentSessionRow>) {
    let mut delivered: HashSet<_> = existing.iter().map(|row| row.session_ref.clone()).collect();
    incoming.retain(|row| delivered.insert(row.session_ref.clone()));
    incoming.sort_by(row_order);
    // Never insert before an already-issued cursor when a later batch
    // discovers a newer record; that would repeat or hide another row.
    existing.extend(incoming);
}

fn row_order(left: &AgentSessionRow, right: &AgentSessionRow) -> std::cmp::Ordering {
    session_sort_key(right.session.updated_at.as_deref())
        .cmp(&session_sort_key(left.session.updated_at.as_deref()))
        .then_with(|| left.session_ref.cmp(&right.session_ref))
}

fn scan_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    workspaces: &[AgentSessionWorkspace],
    text: &str,
    index_config: &SessionIndexConfig,
    budget: &SessionReadBudget,
    previous_matches: Arc<Mutex<HashMap<String, bool>>>,
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
                index_state: if index_config.enabled {
                    CliSessionIndexState::Fallback
                } else {
                    CliSessionIndexState::Disabled
                },
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
    let loaded = match super::budget::with_source_root(&config.config_root, || {
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
            outcome.can_continue = error.contains("预算");
            outcome.states = failure(error, AgentSessionSourceStatus::Partial);
            return outcome;
        }
    };
    let index = (index_config.enabled && !query.is_empty())
        .then(|| HistoryIndex::new(index_config, source.agent_kind, &source.id));
    for loaded in loaded {
        let Some(workspace) = workspaces.iter().find(|workspace| {
            path_identity(std::path::Path::new(&workspace.path)) == path_identity(&loaded.workdir)
        }) else {
            continue;
        };
        let mut state = AgentSessionSourceState {
            source_id: source.id.clone(),
            workspace_id: workspace.id.clone(),
            state: if loaded.complete {
                AgentSessionSourceStatus::Complete
            } else {
                AgentSessionSourceStatus::Partial
            },
            loaded_count: loaded.records.len(),
            message: (!loaded.diagnostics.is_empty()).then(|| loaded.diagnostics.join("；")),
            index_state: if index_config.enabled {
                CliSessionIndexState::Ready
            } else {
                CliSessionIndexState::Disabled
            },
        };
        // Native duplicate indexes are merged by the adapter's project/record
        // identity, not by a UI title, install path or bare cross-source ID.
        for record in super::super::normalize_records(loaded.records) {
            let row = row_for_record(source, config, workspace, &record);
            let ref_key = row.session_ref.clone();
            let prior = previous_matches
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&ref_key)
                .copied();
            let matches = prior.map(Ok).unwrap_or_else(|| {
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
                    let content = match &index {
                        Some(index) => {
                            match index.search(&workspace.id, &record, adapter, &request, budget) {
                                Ok(content) => content,
                                Err(error) => {
                                    state.index_state = CliSessionIndexState::Fallback;
                                    append_state_message(
                                        &mut state,
                                        format!("索引暂不可用，已改用原生正文扫描：{error}"),
                                    );
                                    (adapter.search)(&record, &request, &|| budget.check().is_ok())?
                                }
                            }
                        }
                        None => (adapter.search)(&record, &request, &|| budget.check().is_ok())?,
                    };
                    matched.merge_content(content);
                }
                Ok(matched.complete())
            });
            match matches {
                Ok(value) => {
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
                Err(error) => {
                    state.state = AgentSessionSourceStatus::Partial;
                    append_state_message(&mut state, error);
                }
            }
            outcome.references.push(ReferenceState { row, record });
        }
        if budget.check().is_err() && !budget.cancelled.load(Ordering::Acquire) {
            outcome.can_continue = true;
            state.state = AgentSessionSourceStatus::Partial;
            append_state_message(&mut state, "本轮读取预算已用完，可继续读取其余会话".into());
        }
        if budget.cancelled.load(Ordering::Acquire) {
            state.state = AgentSessionSourceStatus::Cancelled;
        }
        outcome.states.push(state);
    }
    if let Some(index) = index {
        if let Err(error) = index.finish(budget) {
            for state in &mut outcome.states {
                state.index_state = CliSessionIndexState::Fallback;
                state.state = if budget.cancelled.load(Ordering::Acquire) {
                    AgentSessionSourceStatus::Cancelled
                } else {
                    AgentSessionSourceStatus::Partial
                };
                append_state_message(state, format!("索引容量维护未完成：{error}"));
            }
            // Maintenance alone does not mean native records remain unread.
            // Preserve an existing continuation, but do not create an endless
            // cursor just to retry capacity work after native scanning finished.
        }
    }
    outcome
}

fn append_state_message(state: &mut AgentSessionSourceState, message: String) {
    if let Some(previous) = &mut state.message {
        if !previous.contains(&message) {
            previous.push('；');
            previous.push_str(&message);
        }
    } else {
        state.message = Some(message);
    }
}

fn row_for_record(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    workspace: &AgentSessionWorkspace,
    record: &SessionHistoryRecord,
) -> AgentSessionRow {
    let mut session = record.summary.clone();
    let resume_reason = record.resume_reason.clone().or_else(|| {
        if !workspace.exists {
            Some("工作目录不存在或未挂载；仍可查看历史".into())
        } else {
            config.resume_reason.clone()
        }
    });
    session.can_resume &= resume_reason.is_none();
    let parent = match &record.parent_native_id {
        Some(native_id) => AgentSessionParent::Known {
            native_id: native_id.clone(),
            parent_ref: None,
        },
        None if record.role == AgentSessionRole::Main => AgentSessionParent::None,
        None => AgentSessionParent::Unknown,
    };
    AgentSessionRow {
        session_ref: super::scope::session_identity(source, workspace, record),
        source_id: source.id.clone(),
        workspace_id: workspace.id.clone(),
        session,
        role: record.role,
        parent,
        resume_reason,
        activity_state: AgentSessionActivityState::Unknown,
        runtime_ids: Vec::new(),
    }
}
fn apply_activity(rows: &mut [AgentSessionRow]) {
    let activity = super::super::resume::native_session_activity();
    let wanted: HashSet<_> = rows.iter().map(|row| row.session_ref.as_str()).collect();
    let identities: HashMap<_, _> = {
        let references = state::registry();
        references
            .references
            .values()
            .filter(|entry| wanted.contains(entry.row.session_ref.as_str()))
            .map(|entry| {
                (
                    entry.row.session_ref.clone(),
                    hash(&[
                        "native",
                        &entry.row.source_id,
                        &entry.row.workspace_id,
                        &entry.record.record_key,
                    ]),
                )
            })
            .collect()
    };
    for row in rows {
        let Some(source_identity) = identities.get(&row.session_ref) else {
            continue;
        };
        row.runtime_ids = activity
            .iter()
            .filter(|item| item.matches(row.session.cli_kind, source_identity, &row.session.id))
            .map(|item| item.runtime_id.clone())
            .collect();
        row.activity_state = if row.runtime_ids.is_empty() {
            AgentSessionActivityState::Unknown
        } else {
            AgentSessionActivityState::Active
        };
    }
}

fn agent_counts(
    snapshot: &Snapshot,
    sources: &[AgentSessionSource],
    workspaces: &[AgentSessionWorkspace],
) -> Vec<AgentSessionCount> {
    super::counts::summarize(
        snapshot
            .rows
            .iter()
            .map(|row| (row.session.cli_kind, row.session_ref.as_str())),
        &snapshot.states,
        sources,
        workspaces,
    )
}

fn page(
    id: &str,
    snapshot: &Snapshot,
    offset: usize,
    size: usize,
    sources: &[AgentSessionSource],
    workspaces: &[AgentSessionWorkspace],
) -> AgentSessionPage {
    let end = offset.saturating_add(size).min(snapshot.rows.len());
    let items = snapshot.rows[offset..end].to_vec();
    // Cursors already prove which rows were delivered. Only those corrected
    // rows need a separate update; current-page items carry their latest parent.
    // Keep the latest correction so retrying the same cursor remains idempotent.
    let parent_updates = snapshot.rows[..offset]
        .iter()
        .filter_map(|row| {
            snapshot
                .parent_updates
                .get(&row.session_ref)
                .map(|parent| AgentSessionParentUpdate {
                    session_ref: row.session_ref.clone(),
                    parent: parent.clone(),
                })
        })
        .collect();
    AgentSessionPage {
        snapshot_id: id.into(),
        scope_revision: snapshot.scope_revision.clone(),
        items,
        parent_updates,
        next_cursor: (end < snapshot.rows.len() || snapshot.progress.can_continue)
            .then(|| format!("{id}:{end}")),
        loaded_count: snapshot.rows.len(),
        total: snapshot.complete.then_some(snapshot.rows.len()),
        agent_counts: agent_counts(snapshot, sources, workspaces),
        source_states: snapshot.states.clone(),
    }
}

pub(crate) fn detail(
    actor: AgentSessionActor<'_>,
    request: &AgentSessionDetailRequest,
    budget: &SessionReadBudget,
) -> Result<AgentSessionDetail, String> {
    let target = super::resolve_session_ref(actor, &request.scope_revision, &request.session_ref)?;
    let reference = state::registry()
        .references
        .get(&(
            actor.key(),
            request.scope_revision.clone(),
            request.session_ref.clone(),
        ))
        .cloned()
        .ok_or_else(|| "会话引用已失效".to_string())?;
    let mut detail = with_read_budget(budget, || {
        super::budget::with_source_root(&target.config_root, || {
            (history_adapter(target.cli_kind)?.detail)(
                &reference.record,
                super::super::DETAIL_READ_LIMITS,
            )
        })
    })?;
    budget.check()?;
    super::resolve_session_ref(actor, &request.scope_revision, &request.session_ref)?;
    let mut row = reference.row;
    apply_activity(std::slice::from_mut(&mut row));
    detail.session = row.session.clone();
    Ok(AgentSessionDetail { row, detail })
}

#[cfg(test)]
mod tests;
