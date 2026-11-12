//! Native session scope and query ownership. Native parsing stays in the registry adapters.
mod budget;
mod counts;
mod handoff;
mod project_report;
mod query;
mod reader;
mod scope;
mod state;
mod usage_report;
mod workspace_scan;

#[cfg(test)]
pub(crate) use budget::with_history_cache;

use crate::services::agent_cli::contracts::SessionReadBudget;
pub(crate) use budget::{
    cached_history_record_facts, cached_history_record_with_mode, check_read_budget,
    open_session_file, read_session_metadata_text_file_limited, read_session_text_file_limited,
    with_read_budget, with_source_root, BudgetFile, HistoryRecordFacts,
};
pub(crate) use counts::count;
pub(crate) use handoff::prepare_handoff;
pub(crate) use project_report::{collect_project_report, finalize_project_report};
pub(crate) use query::query;
pub(crate) use reader::{detail, message_content, search};
#[cfg(test)]
pub(crate) use scope::ResumeAdmissionFixture;
pub(crate) use scope::{get_scope, relocation_source, resolve_session_ref, ResolvedSessionTarget};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
pub(crate) use usage_report::{collect_usage_report, finalize_usage_report, validate_usage_period};

pub(crate) const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const DETAIL_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const SEARCH_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const COUNT_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const USAGE_REPORT_TIMEOUT: Duration = Duration::from_secs(45);
pub(crate) const PROJECT_REPORT_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const HANDOFF_TIMEOUT: Duration = Duration::from_secs(20);

/// 项目视图的进度回调。服务层只产生结构化进度，宿主负责转发到 IPC 或事件流。
pub(crate) type ProjectProgressReporter =
    std::sync::Arc<dyn Fn(crate::models::AgentProjectProgress) + Send + Sync>;

fn emit_progress(
    reporter: &Option<ProjectProgressReporter>,
    phase: crate::models::AgentProjectProgressPhase,
    completed: u64,
    total: u64,
    message: impl Into<String>,
) {
    if let Some(reporter) = reporter {
        reporter(crate::models::AgentProjectProgress {
            phase,
            completed,
            total,
            message: message.into(),
        });
    }
}

/// 会话读取的调用方身份：桌面传窗口标签，浏览器版传客户端标识。
#[derive(Clone, Copy)]
pub(crate) struct AgentSessionActor<'a> {
    pub client: &'a str,
}

impl AgentSessionActor<'_> {
    fn key(self) -> String {
        self.client.to_owned()
    }
}

pub(crate) fn begin_request(
    actor: AgentSessionActor<'_>,
    consumer: &str,
    generation: u64,
    timeout: Duration,
) -> Result<SessionReadBudget, String> {
    begin_request_key(&actor.key(), consumer, generation, timeout)
}

fn begin_request_key(
    actor_key: &str,
    consumer: &str,
    generation: u64,
    timeout: Duration,
) -> Result<SessionReadBudget, String> {
    if consumer.is_empty() || consumer.len() > 128 || consumer.chars().any(char::is_control) {
        return Err("会话调用方标识无效".into());
    }
    let mut registry = state::registry();
    let key = (actor_key.to_string(), consumer.to_string());
    if let Some(old) = registry.requests.get(&key) {
        if generation <= old.generation {
            return Err("会话请求已过期".into());
        }
        old.cancelled.store(true, Ordering::Release);
    }
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    registry.requests.insert(
        key,
        state::RequestState {
            generation,
            cancelled: cancelled.clone(),
        },
    );
    Ok(SessionReadBudget::new(
        Instant::now() + timeout,
        cancelled,
        512 * 1024 * 1024,
    ))
}

pub(crate) fn cancel(actor: AgentSessionActor<'_>, consumer: &str, generation: u64) {
    if let Some(request) = state::registry()
        .requests
        .get(&(actor.key(), consumer.into()))
    {
        if request.generation == generation {
            request.cancelled.store(true, Ordering::Release);
        }
    }
}

pub(crate) fn release_window(window_label: &str) {
    let actor = AgentSessionActor {
        client: window_label,
    }
    .key();
    let mut registry = state::registry();
    registry.requests.retain(|(owner, _), request| {
        if owner != &actor {
            return true;
        }
        request.cancelled.store(true, Ordering::Release);
        false
    });
    registry.scopes.retain(|(owner, _), _| owner != &actor);
    registry
        .references
        .retain(|(owner, _, _), _| owner != &actor);
    registry
        .snapshots
        .retain(|_, snapshot| snapshot.actor != actor);
    registry.sources.retain(|(owner, _), _| owner != &actor);
}

pub(crate) fn invalidate_source(root: &std::path::Path) {
    let mut registry = state::registry();
    let source_ids = registry
        .scopes
        .values()
        .flat_map(|scope| {
            scope
                .sources
                .iter()
                .filter(|(_, source)| source.config_root == root)
                .map(|(id, _)| id.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let actors = registry
        .scopes
        .iter()
        .filter(|(_, scope)| scope.sources.keys().any(|id| source_ids.contains(id)))
        .map(|((actor, _), _)| actor.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for ((actor, _), request) in &registry.requests {
        if actors.contains(actor) {
            request.cancelled.store(true, Ordering::Release);
        }
    }
    registry
        .sources
        .retain(|(_, id), _| !source_ids.contains(id));
    registry
        .references
        .retain(|_, reference| !source_ids.contains(&reference.row.source_id));
    registry.snapshots.retain(|_, snapshot| {
        !snapshot
            .rows
            .iter()
            .any(|row| source_ids.contains(&row.source_id))
    });
}

pub(super) fn hash(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_generations_are_isolated_by_window_and_consumer() {
        let a = begin_request_key("test-window-a", "history", 1, QUERY_TIMEOUT).unwrap();
        let b = begin_request_key("test-window-b", "history", 1, QUERY_TIMEOUT).unwrap();
        let detail = begin_request_key("test-window-a", "detail", 1, QUERY_TIMEOUT).unwrap();
        let next = begin_request_key("test-window-a", "history", 2, QUERY_TIMEOUT).unwrap();
        assert!(a.check().is_err());
        assert!(b.check().is_ok());
        assert!(detail.check().is_ok());
        assert!(next.check().is_ok());
        assert!(begin_request_key("test-window-a", "history", 1, QUERY_TIMEOUT).is_err());
    }
}
