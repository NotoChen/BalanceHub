//! Native session scope and query ownership. Native parsing stays in the registry adapters.
mod budget;
mod counts;
mod query;
mod scope;
mod state;

use crate::services::agent_cli::contracts::SessionReadBudget;
pub(crate) use budget::{
    cached_history_record, cached_history_record_facts, check_read_budget, open_session_file,
    read_session_metadata_text_file_limited, read_session_text_file_limited,
    session_file_read_limit, with_read_budget, with_source_root, HistoryRecordFacts,
};
pub(crate) use counts::count;
pub(crate) use query::{detail, query};
#[cfg(test)]
pub(crate) use scope::ResumeAdmissionFixture;
pub(crate) use scope::{get_scope, resolve_session_ref, ResolvedSessionTarget};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tauri::AppHandle;

pub(crate) const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const DETAIL_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const COUNT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy)]
pub(crate) struct AgentSessionActor<'a> {
    pub app: &'a AppHandle,
    pub window_label: &'a str,
}

impl AgentSessionActor<'_> {
    fn key(self) -> String {
        format!("{}:{}", self.app.config().identifier, self.window_label)
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

pub(crate) fn release_window(app: &AppHandle, window_label: &str) {
    let actor = AgentSessionActor { app, window_label }.key();
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
