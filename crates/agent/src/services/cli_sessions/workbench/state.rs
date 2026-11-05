use crate::{
    models::{
        AgentSessionParent, AgentSessionRow, AgentSessionScope, AgentSessionSourceState,
        AgentSessionWorkspace, SessionSearchHit,
    },
    services::{
        agent_cli::contracts::{
            SessionHistoryAdapter, SessionHistoryRecord, SessionHistorySource, SessionReadBudget,
            SessionTranscriptSource,
        },
        cli_sessions::{
            record_stream::WindowRequest,
            transcript::{sources_signature, Transcript, MAX_TRANSCRIPT_BYTES},
        },
    },
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
    /// Process-wide transcript cache; every source shares one byte budget.
    pub transcripts: TranscriptCache,
    pub used_at: Instant,
}

const CACHE_BYTES: usize = 256 * 1024 * 1024;
const CACHE_ENTRIES: usize = 8;

/// Reader pages and message expansion revisit the same native transcript.
/// Keep only the few transcripts currently open; list searches never populate
/// this cache, so a workspace-wide search does not retain every body. Large
/// transcripts are cached per byte window, and the LRU is bounded by decoded
/// size as well as entry count so a handful of long sessions cannot pin
/// arbitrary memory. The cache is shared by every source: a per-source budget
/// would multiply the resident ceiling by the number of open workspaces.
#[derive(Clone)]
pub(super) struct TranscriptCache {
    inner: Arc<Mutex<TranscriptCacheData>>,
}

struct TranscriptCacheData {
    entries: lru::LruCache<String, Arc<Transcript>>,
    weights: HashMap<String, usize>,
    bytes: usize,
}

impl Default for TranscriptCache {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TranscriptCacheData {
                entries: lru::LruCache::unbounded(),
                weights: HashMap::new(),
                bytes: 0,
            })),
        }
    }
}

fn shared_transcripts() -> TranscriptCache {
    static CACHE: OnceLock<TranscriptCache> = OnceLock::new();
    CACHE.get_or_init(TranscriptCache::default).clone()
}

impl TranscriptCacheData {
    fn put(&mut self, key: String, transcript: Arc<Transcript>) {
        let weight = transcript.estimated_bytes();
        if let Some(previous) = self.weights.insert(key.clone(), weight) {
            self.bytes = self.bytes.saturating_sub(previous);
        }
        self.entries.put(key, transcript);
        self.bytes = self.bytes.saturating_add(weight);
        // Keep at least the window the caller just asked for, even when that
        // single window exceeds the budget.
        while self.entries.len() > 1
            && (self.bytes > CACHE_BYTES || self.entries.len() > CACHE_ENTRIES)
        {
            let Some((old_key, _)) = self.entries.pop_lru() else {
                break;
            };
            if let Some(old_weight) = self.weights.remove(&old_key) {
                self.bytes = self.bytes.saturating_sub(old_weight);
            }
        }
    }

    fn find_by_revision(&mut self, revision: &str) -> Option<Arc<Transcript>> {
        let key = self
            .entries
            .iter()
            .find(|(_, transcript)| transcript.revision() == revision)
            .map(|(key, _)| key.clone())?;
        self.entries.get(&key).cloned()
    }
}

impl TranscriptCache {
    pub(super) fn load(
        &self,
        workspace: &str,
        record: &SessionHistoryRecord,
        adapter: &SessionHistoryAdapter,
        window: WindowRequest,
        anchor: Option<&str>,
        budget: &SessionReadBudget,
    ) -> Result<Arc<Transcript>, String> {
        let sources = (adapter.transcript)(record)?;
        let signature = sources_signature(&sources)?;
        let prefix = format!("{workspace}\0{}\0{signature}", record.record_key);
        let transcript = self.load_one(
            &prefix,
            workspace,
            &record.record_key,
            &sources,
            window,
            budget,
        )?;
        let Some(anchor) = anchor else {
            return Ok(transcript);
        };
        if transcript.has_message(anchor) {
            return Ok(transcript);
        }
        // A search hit can live outside the newest window. Files within the
        // full-read limit fall back to the whole transcript for this explicit
        // jump; larger files keep the bounded window because their search only
        // covers the newest slice anyway.
        let Some(source_window) = transcript.window else {
            return Ok(transcript);
        };
        if source_window.file_bytes > MAX_TRANSCRIPT_BYTES {
            return Ok(transcript);
        }
        let full = self.load_one(
            &prefix,
            workspace,
            &record.record_key,
            &sources,
            WindowRequest::Default,
            budget,
        )?;
        Ok(if full.has_message(anchor) {
            full
        } else {
            transcript
        })
    }

    fn load_one(
        &self,
        prefix: &str,
        workspace: &str,
        session: &str,
        sources: &[SessionTranscriptSource],
        window: WindowRequest,
        budget: &SessionReadBudget,
    ) -> Result<Arc<Transcript>, String> {
        let key = format!("{prefix}\0{}", window.cache_key());
        {
            let mut data = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(transcript) = data.entries.get(&key) {
                budget.check()?;
                return Ok(transcript.clone());
            }
        }
        let transcript = Arc::new(Transcript::from_sources_with_window(
            workspace, session, sources, window, budget,
        )?);
        // Complete, legacy-truncated and windowed transcripts all hold a
        // bounded slice of the source, so they are safe to reuse.
        if transcript.complete || transcript.truncated || transcript.window.is_some() {
            let mut data = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            data.put(key, transcript.clone());
        }
        Ok(transcript)
    }

    /// Message expansion follows the revision of the page the user is
    /// looking at. Reuse that exact decoded window instead of loading the
    /// newest one when the older page is still cached.
    pub(super) fn find_by_revision(&self, revision: &str) -> Option<Arc<Transcript>> {
        let mut data = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        data.find_by_revision(revision)
    }

    #[cfg(test)]
    fn shares_budget_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

pub(super) fn source_resources(
    actor: super::AgentSessionActor<'_>,
    source: &crate::models::AgentSessionSource,
) -> SourceResources {
    let mut registry = registry();
    let key = (actor.key(), source.id.clone());
    if let Some(resource) = registry.sources.get_mut(&key) {
        resource.used_at = Instant::now();
        return resource.clone();
    }
    let resource = SourceResources {
        cache: Default::default(),
        transcripts: shared_transcripts(),
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
