//! Count native identities across the same source/workspace scope as history.
//! No pagination, transcript search, resume references or persisted partial sums.
use super::{
    budget::{with_history_cache, with_read_budget, with_source_root},
    scope::{build_scope, history_adapter, path_identity, session_identity},
    AgentSessionActor,
};
use crate::{
    models::*,
    services::agent_cli::contracts::{
        SessionHistoryReadMode, SessionHistorySource, SessionReadBudget,
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

struct CountSource {
    identities: Vec<(AgentCliKind, String)>,
    states: Vec<AgentSessionSourceState>,
}

pub(crate) fn count(
    actor: AgentSessionActor<'_>,
    request: &AgentSessionCountRequest,
    budget: &SessionReadBudget,
) -> Result<AgentSessionCounts, String> {
    budget.check()?;
    let scope = build_scope(actor, None)?;
    let sources = scope
        .public
        .sources
        .iter()
        .filter(|source| {
            request.agent_kinds.is_empty() || request.agent_kinds.contains(&source.agent_kind)
        })
        .cloned()
        .collect::<Vec<_>>();
    let workspaces = &scope.public.workspaces;
    let results = std::thread::scope(|threads| {
        let mut handles = Vec::new();
        for source in &sources {
            let config = scope.sources.get(&source.id).ok_or("原生会话来源已失效")?;
            let source_budget =
                SessionReadBudget::new(budget.deadline, budget.cancelled.clone(), budget.max_bytes);
            handles.push(threads.spawn(move || {
                let cache = Default::default();
                with_read_budget(&source_budget, || {
                    with_history_cache(&cache, || {
                        with_source_root(&config.config_root, || {
                            read_source(source, config, workspaces, &source_budget)
                        })
                    })
                })
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "会话统计线程异常".to_owned()))
            .collect::<Result<Vec<_>, String>>()
    })?;
    budget.check()?;
    if build_scope(actor, None)?.public.revision != scope.public.revision {
        return Err("会话统计范围已变化，请重新读取".into());
    }
    let identities = results.iter().flat_map(|result| {
        result
            .identities
            .iter()
            .map(|(kind, id)| (*kind, id.as_str()))
    });
    let states = results
        .iter()
        .flat_map(|result| &result.states)
        .cloned()
        .collect::<Vec<_>>();
    let counts = summarize(identities, &states, &sources, workspaces);
    let mut errors = BTreeMap::new();
    for count in &counts {
        if count.total.is_some() {
            continue;
        }
        let mut messages = Vec::new();
        for state in states.iter().filter(|state| {
            state.state != AgentSessionSourceStatus::Complete
                && sources.iter().any(|source| {
                    source.agent_kind == count.agent_kind && source.id == state.source_id
                })
        }) {
            if let Some(message) = &state.message {
                if !messages.contains(message) {
                    messages.push(message.clone());
                }
            }
        }
        errors.insert(
            count.agent_kind,
            if messages.is_empty() {
                "未能完整读取原生会话索引".into()
            } else {
                messages.join("；")
            },
        );
    }
    Ok(AgentSessionCounts { counts, errors })
}

fn read_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    workspaces: &[AgentSessionWorkspace],
    budget: &SessionReadBudget,
) -> CountSource {
    let empty = |status, message: Option<String>| CountSource {
        identities: Vec::new(),
        states: workspaces
            .iter()
            .map(|workspace| source_state(source, workspace, status, 0, message.clone()))
            .collect(),
    };
    match std::fs::metadata(&config.config_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return empty(AgentSessionSourceStatus::Complete, None)
        }
        Err(error) => {
            return empty(
                AgentSessionSourceStatus::Unavailable,
                Some(format!("原生会话目录不可读：{error}")),
            )
        }
        Ok(metadata) if !metadata.is_dir() => {
            return empty(
                AgentSessionSourceStatus::Unavailable,
                Some("原生会话来源不是目录".into()),
            )
        }
        Ok(_) => {}
    }
    let adapter = match history_adapter(source.agent_kind) {
        Ok(adapter) => adapter,
        Err(error) => return empty(AgentSessionSourceStatus::Unsupported, Some(error)),
    };
    let paths = workspaces
        .iter()
        .map(|workspace| PathBuf::from(&workspace.path))
        .collect::<Vec<_>>();
    let loaded = match (adapter.scan)(
        source.agent_kind,
        &config.config_root,
        &paths,
        budget,
        SessionHistoryReadMode::Identities,
    ) {
        Ok(loaded) => loaded,
        Err(error) => return empty(AgentSessionSourceStatus::Partial, Some(error)),
    };
    let mut result = CountSource {
        identities: Vec::new(),
        states: Vec::new(),
    };
    for loaded in loaded {
        let Some(workspace) = workspaces.iter().find(|workspace| {
            path_identity(Path::new(&workspace.path)) == path_identity(&loaded.workdir)
        }) else {
            continue;
        };
        result.states.push(source_state(
            source,
            workspace,
            if loaded.complete {
                AgentSessionSourceStatus::Complete
            } else {
                AgentSessionSourceStatus::Partial
            },
            loaded.records.len(),
            (!loaded.diagnostics.is_empty()).then(|| loaded.diagnostics.join("；")),
        ));
        for record in super::super::normalize_records(loaded.records) {
            result.identities.push((
                source.agent_kind,
                session_identity(source, workspace, &record),
            ));
        }
    }
    result
}

fn source_state(
    source: &AgentSessionSource,
    workspace: &AgentSessionWorkspace,
    state: AgentSessionSourceStatus,
    loaded_count: usize,
    message: Option<String>,
) -> AgentSessionSourceState {
    AgentSessionSourceState {
        source_id: source.id.clone(),
        workspace_id: workspace.id.clone(),
        state,
        loaded_count,
        message,
        index_state: CliSessionIndexState::Disabled,
    }
}

pub(super) fn summarize<'a>(
    identities: impl IntoIterator<Item = (AgentCliKind, &'a str)>,
    states: &[AgentSessionSourceState],
    sources: &[AgentSessionSource],
    workspaces: &[AgentSessionWorkspace],
) -> Vec<AgentSessionCount> {
    let mut loaded = HashMap::<AgentCliKind, HashSet<&str>>::new();
    for (kind, identity) in identities {
        loaded.entry(kind).or_default().insert(identity);
    }
    let mut complete_pairs = HashMap::<_, bool>::new();
    for state in states {
        let complete = state.state == AgentSessionSourceStatus::Complete;
        complete_pairs
            .entry((state.source_id.as_str(), state.workspace_id.as_str()))
            .and_modify(|previous| *previous &= complete)
            .or_insert(complete);
    }
    let mut included = HashSet::new();
    sources
        .iter()
        .filter(|source| included.insert(source.agent_kind))
        .map(|source| {
            let loaded_count = loaded.get(&source.agent_kind).map_or(0, HashSet::len);
            let complete = !workspaces.is_empty()
                && sources
                    .iter()
                    .filter(|candidate| candidate.agent_kind == source.agent_kind)
                    .all(|candidate| {
                        workspaces.iter().all(|workspace| {
                            complete_pairs.get(&(candidate.id.as_str(), workspace.id.as_str()))
                                == Some(&true)
                        })
                    });
            AgentSessionCount {
                agent_kind: source.agent_kind,
                loaded_count,
                total: complete.then_some(loaded_count),
            }
        })
        .collect()
}
