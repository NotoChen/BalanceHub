//! Read views consume an already published catalog. Mutation plans still inspect afresh.
use super::CatalogService;
use crate::{
    models::*,
    services::agent_cli::environment::{self, mutation::MutationInventory},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_PUBLICATIONS: usize = 8;

pub(crate) struct PublishedCatalog {
    pub(crate) catalog: AgentAssetCatalog,
    pub(crate) snapshot: Arc<MutationInventory>,
    access_evidence: Vec<environment::access_registry::AgentSourceAccessEvidence>,
    pub(crate) inputs: super::inputs::CatalogInputs,
    refreshed_agents: Mutex<BTreeMap<AgentCliKind, Arc<MutationInventory>>>,
    sequence: u64,
}

impl PublishedCatalog {
    pub(crate) fn inventory_build(&self) -> environment::InventoryBuild {
        environment::InventoryBuild {
            inventory: self.snapshot.inventory.clone(),
            access_evidence: self.access_evidence.clone(),
        }
    }

    pub(crate) fn needs_agent_refresh(&self, kind: AgentCliKind) -> bool {
        let snapshot = self.snapshot_for(kind);
        snapshot
            .inventory
            .contexts
            .iter()
            .filter(|context| context.agent_kind == kind)
            .any(|context| {
                snapshot
                    .inventory
                    .sources
                    .iter()
                    .any(|source| source.context_id == context.id && source.allowed_root.is_empty())
            })
    }

    pub(crate) fn snapshot_for(&self, kind: AgentCliKind) -> Arc<MutationInventory> {
        self.refreshed_agents
            .lock()
            .ok()
            .and_then(|agents| agents.get(&kind).cloned())
            .unwrap_or_else(|| Arc::clone(&self.snapshot))
    }

    pub(crate) fn snapshot_for_kinds(&self, kinds: &BTreeSet<AgentCliKind>) -> MutationInventory {
        let mut result = MutationInventory {
            inventory: self.snapshot.inventory.clone(),
            source_anchors: self.snapshot.source_anchors.clone(),
        };
        if let Ok(agents) = self.refreshed_agents.lock() {
            for kind in kinds {
                if let Some(snapshot) = agents.get(kind) {
                    super::refresh::merge_inventory(
                        &mut result.inventory,
                        snapshot.inventory.clone(),
                        &BTreeSet::from([*kind]),
                    );
                    result
                        .source_anchors
                        .extend(snapshot.source_anchors.clone());
                }
            }
        }
        let sources = result
            .inventory
            .sources
            .iter()
            .map(|source| source.id.as_str())
            .collect::<BTreeSet<_>>();
        result
            .source_anchors
            .retain(|id, _| sources.contains(id.as_str()));
        result
    }

    pub(crate) fn remember_agent(
        &self,
        kind: AgentCliKind,
        snapshot: MutationInventory,
    ) -> Arc<MutationInventory> {
        let snapshot = Arc::new(snapshot);
        if let Ok(mut agents) = self.refreshed_agents.lock() {
            agents.insert(kind, Arc::clone(&snapshot));
        }
        snapshot
    }
}

struct ActiveRead {
    id: String,
    control: ReadControl,
}

#[derive(Default)]
pub(super) struct ReadingState {
    publications: BTreeMap<(String, Option<String>), Arc<PublishedCatalog>>,
    active: BTreeMap<(String, &'static str), ActiveRead>,
    canceled: BTreeMap<(String, String), Instant>,
}

#[derive(Clone)]
pub(crate) struct ReadControl {
    canceled: Arc<AtomicBool>,
    cancellation: Arc<tokio::sync::Notify>,
    deadline: Instant,
}

impl ReadControl {
    pub(crate) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.canceled)
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        if self.canceled.load(Ordering::Relaxed) {
            return Err("资源读取已取消".to_owned());
        }
        if Instant::now() >= self.deadline {
            return Err("读取资源内容超时，请重试".to_owned());
        }
        Ok(())
    }

    fn cancel(&self) {
        self.canceled.store(true, Ordering::Relaxed);
        self.cancellation.notify_one();
    }
}

impl CatalogService {
    /// The permit stays with the worker, including after an IPC timeout. A slow
    /// filesystem cannot accumulate an unbounded queue of blocking scans.
    pub(crate) async fn run_read<T, F>(
        &self,
        actor: &str,
        lane: &'static str,
        id: &str,
        task: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(ReadControl) -> Result<T, String> + Send + 'static,
    {
        let control = self.begin_read(actor, lane, id)?;
        let worker_control = control.clone();
        let work = async {
            control.check()?;
            let permit = tokio::select! {
                _ = control.cancellation.notified() => return Err("资源读取已取消".to_owned()),
                permit = Arc::clone(&self.read_slots).acquire_owned() => permit.map_err(|_| "资源读取服务已关闭")?,
            };
            tauri::async_runtime::spawn_blocking(move || {
                let _permit = permit;
                worker_control.check()?;
                task(worker_control)
            })
            .await
            .map_err(|_| "资源读取任务失败".to_owned())?
        };
        let result = tokio::time::timeout(Duration::from_secs(13), work)
            .await
            .map_err(|_| "读取资源超时，请重试".to_owned());
        control.cancel();
        self.finish_read(actor, lane, id);
        result?
    }

    pub(crate) fn publish_read_snapshot(
        &self,
        actor: &str,
        sequence: u64,
        catalog: AgentAssetCatalog,
        build: environment::InventoryBuild,
        inputs: super::inputs::CatalogInputs,
    ) -> Result<(), String> {
        let snapshot = MutationInventory {
            inventory: build.inventory,
            source_anchors: build
                .access_evidence
                .iter()
                .map(|evidence| (evidence.source_id.clone(), evidence.anchor.clone()))
                .collect(),
        };
        let key = (actor.to_owned(), snapshot.inventory.workspace.clone());
        let mut state = self.reading.lock().map_err(|_| "资源读取状态不可用")?;
        if state
            .publications
            .get(&key)
            .is_some_and(|current| current.sequence > sequence)
        {
            return Ok(());
        }
        let refreshed_agents = state
            .publications
            .get(&key)
            .filter(|previous| previous.catalog.revision == catalog.revision)
            .and_then(|previous| {
                previous
                    .refreshed_agents
                    .lock()
                    .ok()
                    .map(|agents| agents.clone())
            })
            .unwrap_or_default();
        state.publications.insert(
            key,
            Arc::new(PublishedCatalog {
                catalog,
                snapshot: Arc::new(snapshot),
                access_evidence: build.access_evidence,
                inputs,
                sequence,
                refreshed_agents: Mutex::new(refreshed_agents),
            }),
        );
        while state.publications.len() > MAX_PUBLICATIONS {
            let oldest = state
                .publications
                .iter()
                .min_by_key(|(_, value)| value.sequence)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                state.publications.remove(&key);
            }
        }
        Ok(())
    }

    pub(crate) fn published_catalog(
        &self,
        actor: &str,
        workspace: Option<&str>,
    ) -> Result<Arc<PublishedCatalog>, String> {
        self.find_publication(actor, workspace)?
            .ok_or_else(|| "当前窗口的资源目录尚未就绪，请重新读取".to_owned())
    }

    pub(crate) fn publication_revision(
        &self,
        actor: &str,
        workspace: Option<&str>,
    ) -> Result<Option<String>, String> {
        Ok(self
            .find_publication(actor, workspace)?
            .map(|publication| publication.catalog.revision.clone()))
    }

    fn find_publication(
        &self,
        actor: &str,
        workspace: Option<&str>,
    ) -> Result<Option<Arc<PublishedCatalog>>, String> {
        let workspace = environment::normalize_optional_workspace(workspace.map(Path::new))?
            .map(|path| path.to_string_lossy().into_owned());
        Ok(self
            .reading
            .lock()
            .map_err(|_| "资源读取状态不可用")?
            .publications
            .get(&(actor.to_owned(), workspace))
            .cloned())
    }

    pub(super) fn begin_read(
        &self,
        actor: &str,
        lane: &'static str,
        id: &str,
    ) -> Result<ReadControl, String> {
        if id.is_empty() || id.len() > 128 {
            return Err("资源读取请求无效".to_owned());
        }
        let now = Instant::now();
        let mut state = self.reading.lock().map_err(|_| "资源读取状态不可用")?;
        state.canceled.retain(|_, expires| *expires > now);
        if state
            .canceled
            .contains_key(&(actor.to_owned(), id.to_owned()))
        {
            return Err("资源读取已取消".to_owned());
        }
        let control = ReadControl {
            canceled: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(tokio::sync::Notify::new()),
            deadline: now + Duration::from_secs(12),
        };
        if let Some(previous) = state.active.insert(
            (actor.to_owned(), lane),
            ActiveRead {
                id: id.to_owned(),
                control: control.clone(),
            },
        ) {
            previous.control.cancel();
        }
        Ok(control)
    }

    pub(crate) fn cancel_read(&self, actor: &str, id: &str) {
        if id.is_empty() || id.len() > 128 {
            return;
        }
        if let Ok(mut state) = self.reading.lock() {
            for ((owner, _), read) in &state.active {
                if owner == actor && read.id == id {
                    read.control.cancel();
                }
            }
            // Cancellation can arrive before the corresponding invoke is dispatched.
            state.canceled.insert(
                (actor.to_owned(), id.to_owned()),
                Instant::now() + Duration::from_secs(30),
            );
            while state.canceled.len() > 128 {
                let oldest = state
                    .canceled
                    .iter()
                    .min_by_key(|(_, expiry)| **expiry)
                    .map(|(key, _)| key.clone());
                if let Some(key) = oldest {
                    state.canceled.remove(&key);
                }
            }
        }
    }

    fn finish_read(&self, actor: &str, lane: &'static str, id: &str) {
        if let Ok(mut state) = self.reading.lock() {
            let key = (actor.to_owned(), lane);
            if state.active.get(&key).is_some_and(|read| read.id == id) {
                state.active.remove(&key);
            }
        }
    }

    pub(super) fn remove_read_actor(&self, actor: &str) {
        if let Ok(mut state) = self.reading.lock() {
            state.publications.retain(|(owner, _), _| owner != actor);
            state.active.retain(|(owner, _), read| {
                if owner != actor {
                    return true;
                }
                read.control.cancel();
                false
            });
            state.canceled.retain(|(owner, _), _| owner != actor);
        }
    }
}

pub(super) fn content_revision(
    asset: &AgentCatalogAsset,
    sources: &BTreeMap<&str, &AgentAssetSource>,
) -> Result<String, String> {
    let bindings = asset
        .bindings
        .iter()
        .map(|binding| {
            let native = &binding.native;
            let mut sources = native
                .source_ids
                .iter()
                .filter_map(|id| sources.get(id.as_str()))
                .map(|source| (&source.id, &source.revision.identity))
                .collect::<Vec<_>>();
            sources.sort_unstable();
            (
                &binding.id,
                &native.revision.identity,
                &native.represented_declaration_ids,
                sources,
            )
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(
        &asset.id,
        &asset.name,
        asset.version,
        &asset.hook,
        bindings,
        &asset.unresolved_targets,
    ))
    .map_err(|_| "资源内容版本无效")?;
    Ok(super::digest(&bytes))
}
