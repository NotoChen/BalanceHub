use crate::{
    models::{
        AgentSessionParent, AgentSessionRow, AgentSessionScope, AgentSessionSourceState,
        AgentSessionWorkspace, SessionSearchHit,
    },
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
    pub workspaces: Vec<AgentSessionWorkspace>,
    pub complete: bool,
    pub progress: QueryProgress,
    pub used_at: Instant,
}
pub(super) type PreviousMatches = Arc<Mutex<HashMap<String, (bool, Option<SessionSearchHit>)>>>;

#[derive(Clone, Default)]
pub(super) struct QueryProgress {
    pub matches: PreviousMatches,
    pub can_continue: bool,
    pub passes: u32,
}

#[derive(Clone)]
pub(super) struct SourceResources {
    pub cache: super::budget::HistoryReadCache,
    pub index: Arc<crate::services::cli_sessions::index::HistoryIndex>,
    pub used_at: Instant,
}

pub(super) fn source_resources(
    actor: super::AgentSessionActor<'_>,
    source: &crate::models::AgentSessionSource,
    config: &crate::services::cli_sessions::index::SessionIndexConfig,
) -> SourceResources {
    let mut registry = registry();
    let key = (actor.key(), source.id.clone());
    if let Some(resource) = registry.sources.get_mut(&key) {
        if resource.index.matches_config(config) {
            resource.used_at = Instant::now();
            return resource.clone();
        }
    }
    let resource = SourceResources {
        cache: Default::default(),
        index: Arc::new(crate::services::cli_sessions::index::HistoryIndex::new(
            config,
            source.agent_kind,
            &source.id,
        )),
        used_at: Instant::now(),
    };
    registry.sources.insert(key, resource.clone());
    registry.prune();
    resource
}
#[derive(Default)]
pub(super) struct Registry {
    pub requests: HashMap<(String, String), RequestState>,
    pub scopes: HashMap<(String, String), ScopeState>,
    pub references: HashMap<(String, String, String), ReferenceState>,
    pub snapshots: HashMap<String, Snapshot>,
    pub sources: HashMap<(String, String), SourceResources>,
    pub path_identities: HashMap<String, String>,
}
impl Registry {
    pub fn register_scope(&mut self, actor: String, scope: ScopeState) {
        // A visible list keeps its scope until that context changes or its
        // window closes. Native paths are still revalidated on every read.
        self.scopes.retain(|(owner, _), saved| {
            owner != &actor
                || saved.explicit_workdir != scope.explicit_workdir
                || saved.public.revision == scope.public.revision
        });
        let key = (actor, scope.public.revision.clone());
        let mut scope = scope;
        if let Some(previous) = self.scopes.get(&key) {
            for workspace in &previous.public.workspaces {
                if !scope
                    .public
                    .workspaces
                    .iter()
                    .any(|item| item.id == workspace.id)
                {
                    scope.public.workspaces.push(workspace.clone());
                }
            }
        }
        self.scopes.insert(key, scope);
        self.prune();
    }

    pub fn store_snapshot(&mut self, id: String, mut snapshot: Snapshot) {
        // Keep the current cursor across long reading pauses. A new query
        // retires only the previous snapshot owned by the same list consumer.
        self.snapshots.retain(|_, saved| {
            saved.actor != snapshot.actor || saved.consumer != snapshot.consumer
        });
        snapshot.used_at = Instant::now();
        self.snapshots.insert(id, snapshot);
        self.prune();
    }

    pub fn prune(&mut self) {
        self.sources
            .retain(|_, resource| resource.used_at.elapsed() < Duration::from_secs(60 * 30));
        while self.sources.len() > 16 {
            let oldest = self
                .sources
                .iter()
                .min_by_key(|(_, resource)| resource.used_at)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.sources.remove(&key);
            }
        }
        self.references.retain(|(actor, revision, _), _| {
            self.scopes.contains_key(&(actor.clone(), revision.clone()))
        });
        self.snapshots.retain(|_, snapshot| {
            self.scopes
                .contains_key(&(snapshot.actor.clone(), snapshot.scope_revision.clone()))
        });
        while self.snapshots.len() > 32 {
            if let Some(oldest) = self
                .snapshots
                .iter()
                .min_by_key(|(_, value)| value.used_at)
                .map(|(key, _)| key.clone())
            {
                self.snapshots.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests;
pub(super) fn registry() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(Registry::default()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
