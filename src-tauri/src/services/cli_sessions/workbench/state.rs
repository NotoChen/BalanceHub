use crate::{
    models::{AgentSessionParent, AgentSessionRow, AgentSessionScope, AgentSessionSourceState},
    services::agent_cli::contracts::{SessionHistoryRecord, SessionHistorySource},
};
use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

pub(super) struct RequestState {
    pub generation: u64,
    pub cancelled: Arc<AtomicBool>,
}
#[derive(Clone)]
pub(super) struct ScopeState {
    pub public: AgentSessionScope,
    pub explicit_workdir: Option<String>,
    pub sources: HashMap<String, SessionHistorySource>,
    pub created_at: Instant,
}
#[derive(Clone)]
pub(super) struct ReferenceState {
    pub row: AgentSessionRow,
    pub record: SessionHistoryRecord,
}
#[derive(Clone)]
pub(super) struct Snapshot {
    pub actor: String,
    pub consumer: String,
    pub scope_revision: String,
    pub query_key: String,
    pub rows: Vec<AgentSessionRow>,
    pub parent_updates: HashMap<String, AgentSessionParent>,
    pub states: Vec<AgentSessionSourceState>,
    pub complete: bool,
    pub progress: QueryProgress,
    pub created_at: Instant,
}
#[derive(Clone, Default)]
pub(super) struct QueryProgress {
    pub source_caches: HashMap<String, super::budget::HistoryReadCache>,
    pub matches: Arc<Mutex<HashMap<String, bool>>>,
    pub can_continue: bool,
    pub passes: u32,
}
#[derive(Default)]
pub(super) struct Registry {
    pub requests: HashMap<(String, String), RequestState>,
    pub scopes: HashMap<(String, String), ScopeState>,
    pub references: HashMap<(String, String, String), ReferenceState>,
    pub snapshots: HashMap<String, Snapshot>,
    pub path_identities: HashMap<String, String>,
}
impl Registry {
    pub fn prune(&mut self) {
        self.scopes
            .retain(|_, scope| scope.created_at.elapsed() < Duration::from_secs(60 * 60));
        self.references.retain(|(actor, revision, _), _| {
            self.scopes.contains_key(&(actor.clone(), revision.clone()))
        });
        self.snapshots
            .retain(|_, snapshot| snapshot.created_at.elapsed() < Duration::from_secs(10 * 60));
        if self.snapshots.len() >= 32 {
            if let Some(oldest) = self
                .snapshots
                .iter()
                .min_by_key(|(_, value)| value.created_at)
                .map(|(key, _)| key.clone())
            {
                self.snapshots.remove(&oldest);
            }
        }
    }
}
pub(super) fn registry() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(Registry::default()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
