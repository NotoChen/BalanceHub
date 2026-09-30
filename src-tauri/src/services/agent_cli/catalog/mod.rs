//! Global asset identities, private shared definitions and native bindings.
mod action_state;
mod comparison;
mod content_sources;
mod definition;
mod display_cache;
mod hook_definition;
mod hook_receipts;
mod hook_summary;
mod hook_targets;
mod identity;
pub(crate) mod inputs;
mod library_management;
pub(crate) mod mcp_editor;
pub(crate) mod native;
mod observation;
mod operations;
mod options;
mod package;
mod package_cache;
mod panel;
pub(crate) mod plan_scope;
mod planning;
mod presence;
pub(crate) mod projection;
mod reading;
mod refresh;
mod relations;
mod removal;
mod repository;
mod selection;
mod source_index;
mod usage;

use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        token::{opaque_id as native_opaque_id, PlanRegistry},
        MutationInspector, MutationInventory, MutationService,
    },
};
use definition::public_definition;
pub(crate) use reading::{PublishedCatalog, ReadControl};
use repository::{Entry, Repository};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{atomic::AtomicUsize, Arc, Mutex},
};

pub(crate) struct CatalogService {
    #[cfg(test)]
    after_hook_write: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    repository: Repository,
    display_cache_root: Option<PathBuf>,
    package_cache: Mutex<package_cache::PackageCache>,
    pub(crate) publication_slots: Arc<tokio::sync::Semaphore>,
    managed_hook_root: Option<PathBuf>,
    native: Arc<MutationService>,
    revisions: Mutex<BTreeMap<String, (String, String)>>,
    plans: PlanRegistry<Arc<operations::CanonicalPlan>, AgentCatalogAction>,
    relation_plans: PlanRegistry<relations::RelationPlan, AgentCatalogRelationAction>,
    preview_bytes: Arc<AtomicUsize>,
    unpersisted_observations: Mutex<BTreeMap<String, planning::ObservationSeed>>,
    completed_operations: Mutex<BTreeMap<String, (String, AgentCatalogOperation)>>,
    reading: Mutex<reading::ReadingState>,
    read_slots: Arc<tokio::sync::Semaphore>,
    operations: Mutex<BTreeMap<String, Arc<operations::OperationCell>>>,
}

impl CatalogService {
    pub(crate) fn new(root: PathBuf, native: Arc<MutationService>) -> Self {
        Self {
            #[cfg(test)]
            after_hook_write: Mutex::new(None),
            repository: Repository::new(root),
            display_cache_root: None,
            package_cache: Mutex::new(package_cache::PackageCache::default()),
            publication_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            managed_hook_root: None,
            native,
            revisions: Mutex::new(BTreeMap::new()),
            plans: PlanRegistry::default(),
            relation_plans: PlanRegistry::default(),
            preview_bytes: Arc::new(AtomicUsize::new(0)),
            unpersisted_observations: Mutex::new(BTreeMap::new()),
            completed_operations: Mutex::new(BTreeMap::new()),
            reading: Mutex::new(reading::ReadingState::default()),
            read_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            operations: Mutex::new(BTreeMap::new()),
        }
    }
    pub(crate) fn with_display_cache(mut self, root: PathBuf) -> Self {
        self.display_cache_root = Some(root);
        self
    }
    pub(crate) fn with_managed_hook_root(mut self, root: PathBuf) -> Self {
        self.managed_hook_root = Some(root);
        self
    }
    pub(crate) fn remove_actor(&self, actor: &str) {
        self.remove_read_actor(actor);
        self.plans.remove_actor(actor);
        self.relation_plans.remove_actor(actor);
        self.clear_unpersisted_observations();
    }

    fn prune_previews(&self) {
        let now = std::time::Instant::now();
        self.plans.prune_expired(now);
        self.relation_plans.prune_expired(now);
        if let Ok(mut seeds) = self.unpersisted_observations.lock() {
            seeds.retain(|_, seed| seed.expires > now);
        }
    }

    fn clear_unpersisted_observations(&self) {
        if let Ok(mut seeds) = self.unpersisted_observations.lock() {
            seeds.clear();
        }
    }

    pub(crate) fn catalog(
        &self,
        snapshot: &MutationInventory,
    ) -> Result<AgentAssetCatalog, String> {
        let catalog = self
            .repository
            .transact(|library| projection::project(library, snapshot))?;
        self.clear_unpersisted_observations();
        self.finish_catalog(catalog)
    }

    pub(crate) fn catalog_for_publication(
        &self,
        snapshot: &MutationInventory,
        settings: &AppSettings,
        environment_before: &str,
    ) -> Result<(AgentAssetCatalog, inputs::CatalogInputs), String> {
        let mut reader =
            observation::DefinitionReader::with_package_cache(snapshot, &self.package_cache);
        let (catalog, library) = self.repository.transact_observed(|library| {
            projection::project_with(library, snapshot, &mut reader)
        })?;
        self.clear_unpersisted_observations();
        let catalog = self.finish_catalog(catalog)?;
        let inputs = inputs::CatalogInputs::capture(
            snapshot,
            reader.package_anchors(),
            &library,
            settings,
            environment_before,
        );
        Ok((catalog, inputs))
    }

    fn read_catalog(
        &self,
        snapshot: &MutationInventory,
    ) -> Result<(AgentAssetCatalog, repository::LibrarySnapshot), String> {
        self.prune_previews();
        let mut library = self.repository.read_snapshot()?;
        let catalog = if library.guard.bytes().is_none() {
            let workspace = snapshot.inventory.workspace.clone().unwrap_or_default();
            let signature = library.guard.signature();
            let mut seeds = self
                .unpersisted_observations
                .lock()
                .map_err(|_| "观察身份缓存不可用")?;
            if let Some(seed) = seeds.get(&workspace).filter(|seed| {
                seed.guard_signature == signature && seed.expires > std::time::Instant::now()
            }) {
                seed.seed(&mut library.library);
            }
            let mut reader =
                observation::DefinitionReader::with_package_cache(snapshot, &self.package_cache);
            let catalog = projection::project_with(&mut library.library, snapshot, &mut reader)?;
            seeds.remove(&workspace);
            if !library.library.entries.is_empty() {
                seeds.insert(
                    workspace,
                    planning::ObservationSeed::create(
                        &library.library,
                        signature,
                        &self.preview_bytes,
                    )?,
                );
            }
            catalog
        } else {
            self.clear_unpersisted_observations();
            let mut reader =
                observation::DefinitionReader::with_package_cache(snapshot, &self.package_cache);
            projection::project_with(&mut library.library, snapshot, &mut reader)?
        };
        Ok((self.finish_catalog(catalog)?, library))
    }

    fn finish_catalog(&self, mut catalog: AgentAssetCatalog) -> Result<AgentAssetCatalog, String> {
        catalog.counts = super::definitions()
            .iter()
            .map(|definition| (definition.kind, AgentCatalogCounts::default()))
            .collect();
        for asset in &catalog.assets {
            let agents = asset
                .bindings
                .iter()
                .map(|binding| binding.native.agent_kind)
                .chain(
                    asset
                        .unresolved_targets
                        .iter()
                        .map(|target| target.agent_kind),
                )
                .collect::<std::collections::BTreeSet<_>>();
            for kind in agents {
                let counts = catalog.counts.entry(kind).or_default();
                match asset.category {
                    AgentAssetCategory::Skill => counts.skill += 1,
                    AgentAssetCategory::Mcp => counts.mcp += 1,
                    AgentAssetCategory::Plugin | AgentAssetCategory::Extension => {
                        counts.extension += 1
                    }
                    _ => {}
                }
            }
        }
        let sources = catalog
            .inventory
            .sources
            .iter()
            .map(|source| (source.id.as_str(), source))
            .collect();
        for asset in &mut catalog.assets {
            asset.content_revision = reading::content_revision(asset, &sources)?;
        }
        let mut hash = Sha256::new();
        for asset in &catalog.assets {
            hash.update(asset.id.as_bytes());
            hash.update(
                serde_json::to_vec(&(
                    &asset.name,
                    &asset.hook,
                    &asset.created_at,
                    &asset.modified_at,
                ))
                .map_err(|_| "资源展示信息无效")?,
            );
            hash.update(asset.version.unwrap_or_default().to_le_bytes());
            for binding in &asset.bindings {
                hash.update(binding.id.as_bytes());
                hash.update(binding.native.revision.identity.as_bytes());
                if let Some(id) = &binding.variant_id {
                    hash.update(id.as_bytes());
                }
                hash.update(
                    serde_json::to_vec(&(
                        binding.drift,
                        binding.applied_version,
                        &binding.native.actions,
                        &binding.actions,
                        &binding.native.selected_action_installation_id,
                    ))
                    .map_err(|_| "资产绑定版本无效")?,
                );
            }
            hash.update(
                serde_json::to_vec(&asset.unresolved_targets).map_err(|_| "资产应用记录无效")?,
            );
            hash.update(serde_json::to_vec(&asset.application).map_err(|_| "资产应用能力无效")?);
            hash.update(
                serde_json::to_vec(&asset.definition_removal)
                    .map_err(|_| "共享定义删除能力无效")?,
            );
            hash.update(
                serde_json::to_vec(&(&asset.separated_asset_ids, &asset.manual_associations))
                    .map_err(|_| "资产对应关系无效")?,
            );
        }
        for source in &catalog.inventory.sources {
            hash.update(source.revision.identity.as_bytes());
        }
        hash.update(serde_json::to_vec(&catalog.targets).map_err(|_| "资产目标版本无效")?);
        let signature = format!("{:x}", hash.finalize());
        let key = catalog.inventory.workspace.clone().unwrap_or_default();
        let mut revisions = self.revisions.lock().map_err(|_| "资产目录状态失效")?;
        if revisions
            .get(&key)
            .is_none_or(|(previous, _)| previous != &signature)
        {
            revisions.insert(key.clone(), (signature, opaque_id()?));
        }
        catalog.revision = revisions.get(&key).ok_or("目录版本失效")?.1.clone();
        Ok(catalog)
    }

    pub(crate) fn definition(&self, id: &str) -> Result<AgentCatalogDefinition, String> {
        let (name, definition) = self.repository.current(id)?;
        Ok(public_definition(id, &name, &definition))
    }

    pub(crate) fn save(
        &self,
        request: AgentCatalogSaveRequest,
    ) -> Result<AgentCatalogDefinition, String> {
        let definition = self.repository.transact(|library| {
            let id = request.asset_id.clone().map(Ok).unwrap_or_else(opaque_id)?;
            let previous = library.entries.get(&id);
            if request.asset_id.is_some() && previous.is_none() {
                return Err("共享定义不存在".to_owned());
            }
            if previous
                .and_then(Entry::current)
                .map(|definition| definition.version)
                != request.expected_version
            {
                return Err("共享定义版本已变化，请刷新后保存".to_owned());
            }
            if previous.is_some_and(|entry| entry.category != request.category) {
                return Err("不能修改已有资产类别".to_owned());
            }
            let payload = definition::input_payload(&request, previous.and_then(Entry::current))?;
            let entry = library
                .entries
                .entry(id.clone())
                .or_insert_with(|| Entry::new(request.name.clone(), request.category));
            let value = entry.set_definition(&request.name, payload)?;
            Ok(public_definition(&id, &entry.name, &value))
        })?;
        self.clear_unpersisted_observations();
        Ok(definition)
    }

    pub(crate) fn adopt(
        &self,
        request: AgentCatalogAdoptRequest,
        inspector: Arc<dyn MutationInspector>,
    ) -> Result<AgentCatalogDefinition, String> {
        let snapshot = inspector.inspect().map_err(|_| "原生资产盘点失败")?;
        let catalog = self.catalog(&snapshot)?;
        if catalog.revision != request.expected_revision {
            return Err("资产目录已变化，请刷新后重新收录".to_owned());
        }
        let item = catalog
            .assets
            .iter()
            .find(|item| {
                item.bindings
                    .iter()
                    .any(|binding| binding.id == request.binding_id)
            })
            .ok_or("原生绑定不存在")?;
        let asset = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == request.binding_id)
            .ok_or("原生资产不存在")?;
        if let Some(binding) = item
            .bindings
            .iter()
            .find(|binding| binding.id == request.binding_id && !binding.can_adopt)
        {
            return Err(binding
                .reason
                .clone()
                .unwrap_or_else(|| "该绑定当前不能收录".to_owned()));
        }
        let payload = projection::observe_payload(&snapshot, asset)?;
        self.repository.transact(|library| {
            let entry = library.entries.get_mut(&item.id).ok_or("资产目录已变化")?;
            if entry.current().map(|value| value.version) != item.version
                || entry.category != asset.category
            {
                return Err("共享定义在收录前已变化，请重新读取后操作".to_owned());
            }
            let payload = match (&payload, entry.current().map(|value| &value.payload)) {
                (
                    definition::DefinitionPayload::Hook(incoming),
                    Some(definition::DefinitionPayload::Hook(existing)),
                ) => {
                    let mut variants = existing.clone();
                    variants.extend(incoming.clone());
                    definition::DefinitionPayload::Hook(variants)
                }
                _ => payload.clone(),
            };
            let name = if entry.current().is_none() && entry.category == AgentAssetCategory::Hook {
                item.name.clone()
            } else {
                entry.name.clone()
            };
            let value = entry.set_definition(&name, payload)?;
            Ok(public_definition(&item.id, &entry.name, &value))
        })
    }
}

pub(super) fn opaque_id() -> Result<String, String> {
    native_opaque_id().map_err(|_| "无法生成安全标识".to_owned())
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod hook_acceptance;
#[cfg(all(test, unix))]
mod hook_configuration_only_tests;
#[cfg(all(test, unix))]
mod hook_policy_tests;
#[cfg(all(test, unix))]
mod hook_tests;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod native_acceptance;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod native_batch_tests;
#[cfg(all(test, unix))]
mod operation_tests;
#[cfg(test)]
mod review_regressions;
#[cfg(all(test, unix))]
mod target_regressions;
#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod flow_regressions;
#[cfg(all(test, unix))]
mod relation_regressions;
