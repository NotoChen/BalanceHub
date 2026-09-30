//! Read-only definition preparation. A confirmation retains its planned identity
//! and actual filesystem baseline, never a whole-library replacement snapshot.
use super::{
    definition::{self, StoredDefinition},
    digest, opaque_id, projection,
    repository::{Entry, Library},
    CatalogService,
};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{GuardedFile, MutationInventory},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

/// Until the first library write, repeat reads retain only opaque identity
/// mappings. Current capabilities, payloads and receipt facts are never cached.
pub(super) struct ObservationSeed {
    pub guard_signature: String,
    pub expires: Instant,
    entries: BTreeMap<String, ObservationSeedEntry>,
    _permit: PreviewPermit,
}

#[derive(serde::Serialize)]
struct ObservationSeedEntry {
    name: String,
    category: AgentAssetCategory,
    aliases: BTreeSet<String>,
    variants: BTreeMap<String, String>,
}

impl ObservationSeed {
    pub fn create(
        library: &Library,
        guard_signature: String,
        used: &Arc<AtomicUsize>,
    ) -> Result<Self, String> {
        let entries = library
            .entries
            .iter()
            .map(|(id, entry)| {
                (
                    id.clone(),
                    ObservationSeedEntry {
                        name: entry.name.clone(),
                        category: entry.category,
                        aliases: entry.aliases.keys().cloned().collect(),
                        variants: entry.variants.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let bytes = serde_json::to_vec(&entries)
            .map_err(|_| "观察身份缓存无法编码")?
            .len()
            .saturating_mul(2);
        Ok(Self {
            guard_signature,
            expires: Instant::now()
                + crate::services::agent_cli::environment::mutation::token::DEFAULT_PLAN_TTL,
            entries,
            _permit: reserve(used, bytes)?,
        })
    }
    pub fn seed(&self, library: &mut Library) {
        for (id, seed) in &self.entries {
            let mut entry = Entry::new(seed.name.clone(), seed.category);
            entry.variants.clone_from(&seed.variants);
            entry.aliases = seed
                .aliases
                .iter()
                .map(|id| {
                    (
                        id.clone(),
                        super::repository::Observation {
                            fingerprint: None,
                            physical_key: None,
                            hook_source: None,
                        },
                    )
                })
                .collect();
            library.entries.insert(id.clone(), entry);
        }
    }
}

const PRIVATE_PREVIEW_LIMIT: usize = 64 * 1024 * 1024;

pub(super) struct PreviewPermit {
    used: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for PreviewPermit {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub(super) fn reserve(used: &Arc<AtomicUsize>, bytes: usize) -> Result<PreviewPermit, String> {
    used.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        current
            .checked_add(bytes)
            .filter(|next| *next <= PRIVATE_PREVIEW_LIMIT)
    })
    .map_err(|_| "预览暂存已达到 64 MiB，请等待现有预览过期后重试".to_owned())?;
    Ok(PreviewPermit {
        used: Arc::clone(used),
        bytes,
    })
}

#[derive(Clone)]
pub(super) struct ResolvedPlanRequest {
    pub asset_id: String,
    pub action: AgentCatalogAction,
    pub target_ids: Vec<String>,
    pub expected_version: Option<u64>,
    pub workspace: Option<String>,
}

pub(super) struct PreparedSource {
    pub item: AgentCatalogAsset,
    pub entry: Entry,
    pub guard: GuardedFile,
    pub native_stamp: String,
    pub change: Option<AgentCatalogDefinitionChange>,
    native_source: Option<(String, String)>,
}

impl PreparedSource {
    pub fn estimate_bytes(&self) -> usize {
        let payload = self.entry.versions.iter().fold(0_usize, |total, version| {
            total.saturating_add(version.payload.comparison_bytes())
        });
        let metadata = serde_json::to_vec(&(
            &self.entry.name,
            self.entry.category,
            &self.entry.aliases,
            &self.entry.variants,
            &self.entry.receipts,
        ))
        .map_or(usize::MAX, |bytes| bytes.len());
        self.guard
            .bytes()
            .map_or(0, <[u8]>::len)
            .saturating_add(payload)
            .saturating_add(metadata)
            .saturating_add(serde_json::to_vec(&self.item).map_or(usize::MAX, |bytes| bytes.len()))
    }

    pub fn revalidate(&self, snapshot: &MutationInventory) -> Result<(), String> {
        self.guard
            .revalidate()
            .map_err(|_| "共享库在预览后已变化，请重新计划")?;
        if native_stamp(snapshot)? != self.native_stamp {
            return Err("原生来源、范围或能力在预览后已变化，请重新计划".to_owned());
        }
        if let Some((binding, fingerprint)) = &self.native_source {
            let asset = snapshot
                .inventory
                .assets
                .iter()
                .find(|asset| &asset.stable_id == binding)
                .ok_or("原生来源已离开当前盘点")?;
            if projection::observe_payload(snapshot, asset)?.fingerprint() != *fingerprint {
                return Err("原生完整定义在预览后已变化，请重新计划".to_owned());
            }
        }
        Ok(())
    }

    pub fn save_into(&self, library: &mut Library) -> Result<(), String> {
        let Some(change) = &self.change else {
            return Ok(());
        };
        let current = library.entries.get(&self.item.id);
        if current.and_then(Entry::current).map(|value| value.version) != change.before_version
            || current.is_some_and(|entry| entry.category != self.item.category)
        {
            return Err("共享定义或归属在确认后已变化，未保存此次版本".to_owned());
        }
        let desired = self.entry.current().cloned().ok_or("计划缺少完整定义")?;
        let entry = library
            .entries
            .entry(self.item.id.clone())
            .or_insert_with(|| {
                let mut entry = Entry::new(self.entry.name.clone(), self.entry.category);
                // Only this planned identity is admitted. Other observed entries
                // from a dry projection are never published by this transaction.
                entry.aliases = self.entry.aliases.clone();
                entry.variants = self.entry.variants.clone();
                entry
            });
        entry.name.clone_from(&self.entry.name);
        entry.push_version(desired)
    }
}

pub(super) fn prepare(
    service: &CatalogService,
    request: &AgentCatalogPlanRequest,
    snapshot: &MutationInventory,
    publication: Option<&super::PublishedCatalog>,
) -> Result<(PreparedSource, AgentAssetCatalog, ResolvedPlanRequest), String> {
    let focused;
    let (mut catalog, mut base) = if let Some(publication) = publication {
        if publication.catalog.revision != request.expected_revision {
            return Err("资产目录已变化，请刷新后重新计划".to_owned());
        }
        let mut base = service.repository.read_snapshot()?;
        focused =
            super::plan_scope::focus(snapshot, &publication.catalog, request, &mut base.library)?;
        let catalog = projection::project(&mut base.library, &focused)?;
        (catalog, base)
    } else {
        focused = MutationInventory {
            inventory: snapshot.inventory.clone(),
            source_anchors: snapshot.source_anchors.clone(),
        };
        service.read_catalog(snapshot)?
    };
    if publication.is_some() {
        catalog.revision.clone_from(&request.expected_revision);
    }
    if catalog.revision != request.expected_revision {
        return Err("资产目录已变化，请刷新后重新计划".to_owned());
    }
    let mut native_source = None;
    let (id, change) = match &request.source {
        AgentCatalogPlanSource::Catalog {
            asset_id,
            expected_version,
        } => {
            let item = catalog
                .assets
                .iter()
                .find(|item| &item.id == asset_id)
                .ok_or("全局资产不存在")?;
            if item.version != *expected_version {
                return Err("共享定义版本已变化".to_owned());
            }
            if request.action == AgentCatalogAction::ApplyDefinition && item.version.is_none() {
                return Err("请选择后端返回的原生来源准备安装计划".to_owned());
            }
            (asset_id.clone(), None)
        }
        AgentCatalogPlanSource::NativeBinding {
            asset_id,
            binding_id,
        } => {
            require_application(request.action)?;
            let item = catalog
                .assets
                .iter()
                .find(|item| &item.id == asset_id)
                .ok_or("原生资产不存在")?;
            if item.version.is_some() {
                return Err("该资产已有共享版本，请选择现有版本或明确编辑".to_owned());
            }
            let binding = item
                .bindings
                .iter()
                .find(|binding| &binding.id == binding_id)
                .ok_or("原生来源不属于此逻辑资产")?;
            if !binding.can_adopt {
                return Err(binding
                    .reason
                    .clone()
                    .unwrap_or_else(|| "当前原生来源不能收录".to_owned()));
            }
            let payload = projection::observe_payload(snapshot, &binding.native)?;
            native_source = Some((binding_id.clone(), payload.fingerprint()));
            let entry = base
                .library
                .entries
                .get_mut(asset_id)
                .ok_or("原生归属已变化")?;
            if entry.category == AgentAssetCategory::Hook {
                entry.name.clone_from(&item.name);
            }
            let change = AgentCatalogDefinitionChange {
                kind: AgentCatalogDefinitionChangeKind::Adopt,
                name: entry.name.clone(),
                before_version: None,
                after_version: 1,
            };
            entry.push_version(StoredDefinition {
                version: 1,
                payload,
            })?;
            (asset_id.clone(), Some(change))
        }
        AgentCatalogPlanSource::Draft {
            definition: request,
        } => {
            let id = request.asset_id.clone().map(Ok).unwrap_or_else(opaque_id)?;
            let previous = base.library.entries.get(&id);
            if request.asset_id.is_some() && previous.is_none() {
                return Err("编辑资产已不存在".to_owned());
            }
            if previous.and_then(Entry::current).map(|value| value.version)
                != request.expected_version
                || previous.is_some_and(|entry| entry.category != request.category)
            {
                return Err("编辑版本或类别已变化，请重新读取".to_owned());
            }
            let payload = definition::input_payload(request, previous.and_then(Entry::current))?;
            let entry = base
                .library
                .entries
                .entry(id.clone())
                .or_insert_with(|| Entry::new(request.name.clone(), request.category));
            let value = entry.set_definition(&request.name, payload)?;
            (
                id,
                (Some(value.version) != request.expected_version).then_some(
                    AgentCatalogDefinitionChange {
                        kind: AgentCatalogDefinitionChangeKind::Save,
                        name: request.name.clone(),
                        before_version: request.expected_version,
                        after_version: value.version,
                    },
                ),
            )
        }
    };
    if matches!(request.source, AgentCatalogPlanSource::Draft { .. }) {
        require_application(request.action)?;
    }
    if change.is_some() {
        // The virtual version is deliberately not published to the revision map.
        catalog = projection::project(&mut base.library, &focused)?;
    }
    let item = catalog
        .assets
        .iter()
        .find(|item| item.id == id)
        .cloned()
        .ok_or("规划身份已变化")?;
    let mut entry = base.library.entries.remove(&id).ok_or("规划来源已变化")?;
    // History remains on disk; a plan retains only the version it can execute.
    entry.versions = entry.versions.pop().into_iter().collect();
    let mut target_ids = request.target_ids.clone();
    if target_ids.is_empty() {
        target_ids = if request.action == AgentCatalogAction::ApplyDefinition {
            item.application
                .targets
                .iter()
                .map(|target| target.id.clone())
                .collect()
        } else {
            item.bindings
                .iter()
                .map(|binding| binding.id.clone())
                .chain(
                    item.unresolved_targets
                        .iter()
                        .map(|target| target.target_id.clone()),
                )
                .collect()
        };
    }
    if target_ids.len() > 64 {
        return Err("可选目标超过 64 项，请先选择具体 Agent 或范围".to_owned());
    }
    let resolved = ResolvedPlanRequest {
        asset_id: id,
        action: request.action,
        target_ids,
        expected_version: entry.current().map(|value| value.version),
        workspace: request.workspace.clone(),
    };
    Ok((
        PreparedSource {
            item,
            entry,
            guard: base.guard,
            native_stamp: native_stamp(snapshot)?,
            change,
            native_source,
        },
        catalog,
        resolved,
    ))
}

fn require_application(action: AgentCatalogAction) -> Result<(), String> {
    if action != AgentCatalogAction::ApplyDefinition {
        return Err("原生来源和编辑草稿仅可用于应用定义计划".to_owned());
    }
    Ok(())
}

pub(super) fn native_stamp(snapshot: &MutationInventory) -> Result<String, String> {
    let mut assets = snapshot.inventory.assets.iter().collect::<Vec<_>>();
    assets.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    let mut sources = snapshot.inventory.sources.iter().collect::<Vec<_>>();
    sources.sort_by(|left, right| left.id.cmp(&right.id));
    let value = (
        &snapshot.inventory.workspace,
        &snapshot.inventory.contexts,
        &snapshot.inventory.capabilities,
        &snapshot.inventory.mechanisms,
        assets
            .iter()
            .map(|asset| {
                (
                    &asset.stable_id,
                    &asset.revision.identity,
                    &asset.actions,
                    &asset.relationships,
                    &asset.resolution,
                    &asset.selected_action_installation_id,
                    asset.effective_state,
                    asset.trust_state,
                )
            })
            .collect::<Vec<_>>(),
        sources
            .iter()
            .map(|source| (&source.id, &source.revision.identity, source.writable))
            .collect::<Vec<_>>(),
    );
    serde_json::to_vec(&value)
        .map(|bytes| digest(&bytes))
        .map_err(|_| "原生计划证据无法编码".to_owned())
}
