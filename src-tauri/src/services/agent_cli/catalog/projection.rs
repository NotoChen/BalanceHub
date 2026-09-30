use super::{
    definition::DefinitionPayload,
    identity::{self, CurrentDefinition, OWNERSHIP_CONFLICT},
    native::NativeCatalogAdapter,
    observation::DefinitionReader,
    repository::{self, Library, Observation},
};
use crate::{
    models::*,
    services::agent_cli::{
        definition,
        environment::{mutation::MutationInventory, stable_id},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

pub(super) use super::observation::observe_payload;

pub(crate) use super::source_index::definition_anchor;
pub(super) use super::source_index::definition_source;

fn merge_source_times(
    item: &mut AgentCatalogAsset,
    source: Option<&AgentAssetSource>,
    snapshot: &MutationInventory,
) {
    let Some(anchor) = source.and_then(|source| snapshot.source_anchors.get(&source.id)) else {
        return;
    };
    let (created, modified) = anchor.file_times();
    let format = |time| {
        chrono::DateTime::<chrono::Utc>::from(time)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    if let Some(created) = created.map(format) {
        if item
            .created_at
            .as_ref()
            .is_none_or(|current| &created < current)
        {
            item.created_at = Some(created);
        }
    }
    if let Some(modified) = modified.map(format) {
        if item
            .modified_at
            .as_ref()
            .is_none_or(|current| &modified > current)
        {
            item.modified_at = Some(modified);
        }
    }
}

fn application(
    entry: &repository::Entry,
    asset: &AgentCatalogAsset,
    observations: &BTreeMap<String, CurrentDefinition>,
    conflicted: bool,
    targets: &[AgentCatalogTarget],
    agent_observations: Vec<AgentCatalogAgentObservation>,
) -> AgentCatalogApplication {
    let source = identity::application_source(entry, asset, observations, conflicted);
    let targets = application_targets(
        source
            .as_ref()
            .map(|(payload, _)| *payload)
            .map_err(Clone::clone),
        &asset.name,
        asset.category,
        targets,
    );
    AgentCatalogApplication {
        available: source.is_ok(),
        source_binding_id: source
            .as_ref()
            .ok()
            .and_then(|(_, binding)| binding.clone()),
        targets,
        observations: agent_observations,
        reason: source.err(),
    }
}

pub(super) fn application_targets(
    source: Result<&DefinitionPayload, String>,
    name: &str,
    category: AgentAssetCategory,
    targets: &[AgentCatalogTarget],
) -> Vec<AgentCatalogTarget> {
    targets
        .iter()
        .filter(|target| target.categories.contains(&category))
        .map(|target| {
            let mut target = target.clone();
            let portability =
                source
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|payload| match payload {
                        DefinitionPayload::Mcp(value) => definition(target.agent_kind)
                            .environment
                            .catalog_adapter()
                            .ok_or_else(|| "该 Agent 没有 MCP 分发映射".to_owned())?
                            .encode(value)
                            .map(|_| ()),
                        DefinitionPayload::Skill(_) => super::definition::validate_name(name),
                        DefinitionPayload::Hook(variants) => variants
                            .get(&target.agent_kind)
                            .ok_or_else(|| "尚未提供此 Agent 的原生 Hook 变体".to_owned())
                            .and_then(|value| {
                                super::hook_definition::validate(target.agent_kind, value)
                            }),
                    });
            target.reason = portability.err().or(target.reason);
            target.available &= target.reason.is_none();
            target
        })
        .collect()
}

pub(super) fn project(
    library: &mut Library,
    snapshot: &MutationInventory,
) -> Result<AgentAssetCatalog, String> {
    project_with(library, snapshot, &mut DefinitionReader::new(snapshot))
}

pub(super) fn project_with(
    library: &mut Library,
    snapshot: &MutationInventory,
    reader: &mut DefinitionReader<'_>,
) -> Result<AgentAssetCatalog, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut remaining_bytes = 64 * 1024 * 1024;
    let mut items = BTreeMap::<String, AgentCatalogAsset>::new();
    let mut diagnostics = Vec::new();
    super::hook_receipts::reconcile(library, reader);
    let mut current_assets = snapshot.inventory.assets.iter().collect::<Vec<_>>();
    current_assets.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    let observations = current_assets
        .iter()
        .map(|asset| {
            (
                asset.stable_id.clone(),
                reader.observe(asset, deadline, &mut remaining_bytes),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let identity = identity::reconcile(library, &snapshot.inventory.assets, &observations)?;
    if !identity.conflicts.is_empty() {
        diagnostics.push(OWNERSHIP_CONFLICT.to_owned());
    }
    for asset in current_assets {
        let observation = &observations[&asset.stable_id];
        let payload = &observation.payload;
        let fingerprint = &observation.fingerprint;
        let id = &identity.bindings[&asset.stable_id];
        let identity_conflict = identity.conflicts.contains(id);
        let source = reader.index.source(asset);
        let entry = library.entries.get_mut(id).ok_or("资产观察归属已变化")?;
        entry.aliases.insert(
            asset.stable_id.clone(),
            Observation {
                fingerprint: fingerprint.clone(),
                physical_key: observation.physical_key.clone(),
                hook_source: (asset.category == AgentAssetCategory::Hook).then(|| {
                    super::hook_receipts::HookObservation {
                        context_id: asset.context_id.clone(),
                        source_id: asset.inspection_source_id.clone(),
                        scope: asset.scope,
                    }
                }),
            },
        );
        let variant_id = fingerprint
            .as_deref()
            .map(|fingerprint| entry.variant(fingerprint))
            .transpose()?;
        let version = entry.current().map(|value| value.version);
        let receipt = entry.receipts.values().find(|receipt| {
            repository::receipt_matches(receipt, asset, source.map(|source| source.path.as_str()))
        });
        let applied_version =
            receipt.and_then(|receipt| (receipt.version > 0).then_some(receipt.version));
        let drift = if identity_conflict {
            AgentCatalogDrift::Unknown
        } else {
            match (receipt, fingerprint) {
                (None, None) => AgentCatalogDrift::Unknown,
                (None, Some(_)) => AgentCatalogDrift::Observed,
                (Some(_), None) => AgentCatalogDrift::Unknown,
                (Some(_), Some(fingerprint))
                    if asset.category == AgentAssetCategory::Mcp
                        && entry.current().is_some_and(|current| {
                            current.payload.fingerprint() == *fingerprint
                        }) =>
                {
                    // Current connection equality wins over prior serialization
                    // fingerprints and policy-only version changes.
                    AgentCatalogDrift::InSync
                }
                (Some(receipt), Some(fingerprint)) if &receipt.fingerprint != fingerprint => {
                    AgentCatalogDrift::Modified
                }
                (Some(receipt), _)
                    if version.is_some()
                        && Some(receipt.version) != version
                        && (asset.category != AgentAssetCategory::Hook
                            || entry
                                .current()
                                .and_then(|value| {
                                    value.payload.binding_fingerprint(asset.agent_kind)
                                })
                                .as_deref()
                                != Some(receipt.fingerprint.as_str())) =>
                {
                    AgentCatalogDrift::UpdateAvailable
                }
                (Some(_), _) => AgentCatalogDrift::InSync,
            }
        };
        let item = items
            .entry(id.clone())
            .or_insert_with(|| AgentCatalogAsset {
                id: id.clone(),
                content_revision: String::new(),
                name: entry.name.clone(),
                category: entry.category,
                created_at: None,
                modified_at: None,
                hook: None,
                provenance: AgentAssetProvenanceSummary::default(),
                ownership: if version.is_some() {
                    AgentCatalogOwnership::Managed
                } else {
                    AgentCatalogOwnership::Observed
                },
                version,
                variants: Vec::new(),
                bindings: Vec::new(),
                unresolved_targets: Vec::new(),
                candidate_ids: Vec::new(),
                separated_asset_ids: Vec::new(),
                manual_associations: Vec::new(),
                application: AgentCatalogApplication::default(),
                definition_removal: AgentCatalogDefinitionRemoval::default(),
            });
        merge_source_times(item, source, snapshot);
        if let Some(variant_id) = &variant_id {
            if !item
                .variants
                .iter()
                .any(|variant| &variant.id == variant_id)
            {
                item.variants.push(AgentCatalogVariant {
                    id: variant_id.clone(),
                    label: format!("配置变体 {}", item.variants.len() + 1),
                    complete: true,
                    summary: payload
                        .as_ref()
                        .map(|payload| payload.summary())
                        .unwrap_or_default(),
                });
            }
        }
        item.bindings.push(AgentCatalogBinding {
            id: asset.stable_id.clone(),
            usage: None,
            native: asset.clone(),
            actions: if asset.category == AgentAssetCategory::Hook {
                let mut actions = super::operations::hook_binding_actions(reader, asset);
                if entry.receipts.values().any(|receipt| {
                    receipt.context_id == asset.context_id
                        && receipt.hook.as_ref().is_some_and(|hook| {
                            hook.pending.is_some()
                                && hook.rule.source_id == asset.inspection_source_id
                        })
                }) {
                    for action in &mut actions {
                        if matches!(
                            action.action,
                            AgentAssetActionKind::Enable
                                | AgentAssetActionKind::Disable
                                | AgentAssetActionKind::Remove
                        ) {
                            action.available = false;
                            action.reason =
                                Some(AgentAssetActionUnavailableReason::SourceUnavailable);
                            action.reload_effect =
                                Some("上次 Hook 提交尚未完成对账，恢复信息已保留".to_owned());
                        }
                    }
                }
                actions
            } else {
                if matches!(
                    asset.category,
                    AgentAssetCategory::Plugin | AgentAssetCategory::Extension
                ) {
                    // Package assets are removed through the owning Agent's
                    // native package manager. Keep the materialized native
                    // action so its availability reflects that Agent and
                    // installation, rather than the file catalog remover.
                    asset.actions.clone()
                } else {
                    let mut actions = asset.actions.clone();
                    actions.retain(|action| action.action != AgentAssetActionKind::Remove);
                    actions.push(super::removal::action(snapshot, asset));
                    actions
                }
            },
            variant_id,
            drift,
            applied_version,
            can_adopt: payload.is_ok() && !identity_conflict,
            reason: if identity_conflict {
                Some(OWNERSHIP_CONFLICT.to_owned())
            } else {
                payload.as_ref().err().cloned()
            },
        });
    }
    for (id, entry) in &library.entries {
        if entry.current().is_some() || !entry.receipts.is_empty() {
            items
                .entry(id.clone())
                .or_insert_with(|| AgentCatalogAsset {
                    id: id.clone(),
                    content_revision: String::new(),
                    name: entry.name.clone(),
                    category: entry.category,
                    created_at: None,
                    modified_at: None,
                    hook: None,
                    ownership: if entry.current().is_some() {
                        AgentCatalogOwnership::Managed
                    } else {
                        AgentCatalogOwnership::Observed
                    },
                    provenance: AgentAssetProvenanceSummary::default(),
                    version: entry.current().map(|value| value.version),
                    variants: Vec::new(),
                    bindings: Vec::new(),
                    unresolved_targets: Vec::new(),
                    candidate_ids: Vec::new(),
                    separated_asset_ids: Vec::new(),
                    manual_associations: Vec::new(),
                    application: AgentCatalogApplication::default(),
                    definition_removal: AgentCatalogDefinitionRemoval::default(),
                });
        }
    }
    let mut targets = targets(&snapshot.inventory);
    super::hook_targets::append_receipt_targets(&mut targets, library, snapshot);
    let mut native_observations =
        super::presence::NativeObservations::new(snapshot, &targets, deadline);
    for (id, entry) in &library.entries {
        let Some(item) = items.get_mut(id) else {
            continue;
        };
        for (target_id, receipt) in &entry.receipts {
            let context = snapshot
                .inventory
                .contexts
                .iter()
                .find(|context| context.id == receipt.context_id);
            if item.bindings.iter().any(|binding| {
                repository::receipt_matches(
                    receipt,
                    &binding.native,
                    reader
                        .index
                        .source(&binding.native)
                        .map(|source| source.path.as_str()),
                )
            }) {
                continue;
            }
            if receipt.hook.is_some() {
                let presence = super::hook_receipts::presence(reader, receipt);
                let (state, drift, message) = match presence {
                    super::hook_receipts::HookPresence::Suspended => (
                        AgentCatalogUnresolvedState::Suspended,
                        if entry
                            .current()
                            .and_then(|value| value.payload.binding_fingerprint(receipt.agent_kind))
                            .is_some_and(|fingerprint| fingerprint != receipt.fingerprint)
                        {
                            AgentCatalogDrift::UpdateAvailable
                        } else {
                            AgentCatalogDrift::InSync
                        },
                        "已停用；完整恢复副本保留在共享资源库，更新共享版本不会自动启用",
                    ),
                    super::hook_receipts::HookPresence::Missing => (
                        AgentCatalogUnresolvedState::Missing,
                        AgentCatalogDrift::Missing,
                        "原生 Hook 已缺失；保留应用记录，重新应用需要明确确认",
                    ),
                    _ => (
                        AgentCatalogUnresolvedState::Unknown,
                        AgentCatalogDrift::Unknown,
                        "Hook 来源存在歧义或上次提交尚未完成对账；已保留恢复信息，不自动重放",
                    ),
                };
                item.unresolved_targets.push(AgentCatalogUnresolvedTarget {
                    target_id: target_id.clone(),
                    context_id: receipt.context_id.clone(),
                    agent_kind: receipt.agent_kind,
                    scope: receipt.scope,
                    drift,
                    applied_version: receipt.version,
                    message: message.to_owned(),
                    state,
                    actions: if state == AgentCatalogUnresolvedState::Suspended {
                        super::operations::hook_suspended_actions(snapshot, receipt)
                    } else {
                        Vec::new()
                    },
                });
                continue;
            }
            let drift = if targets
                .iter()
                .find(|target| &target.id == target_id)
                .is_some_and(|target| {
                    native_observations.receipt_missing(target, entry.category, receipt)
                }) {
                AgentCatalogDrift::Missing
            } else {
                AgentCatalogDrift::Unknown
            };
            item.unresolved_targets.push(AgentCatalogUnresolvedTarget {
                target_id: target_id.clone(),
                context_id: receipt.context_id.clone(),
                agent_kind: context.map_or(receipt.agent_kind, |context| context.agent_kind),
                scope: receipt.scope,
                drift,
                applied_version: receipt.version,
                state: if drift == AgentCatalogDrift::Missing {
                    AgentCatalogUnresolvedState::Missing
                } else {
                    AgentCatalogUnresolvedState::Unknown
                },
                actions: Vec::new(),
                message: if drift == AgentCatalogDrift::Missing {
                    "已应用的原生定义已缺失，可重新选择目标应用共享版本"
                } else {
                    "当前扫描无法证明该已应用目标存在或缺失；保留上次应用记录"
                }
                .to_owned(),
            });
        }
    }
    for item in items.values_mut() {
        let entry = &library.entries[&item.id];
        let preserve_name = library.relations.has_association(&item.id);
        super::hook_summary::project(
            item,
            entry,
            &observations,
            &snapshot.inventory,
            preserve_name,
        );
    }
    let mut names = BTreeMap::<_, Vec<String>>::new();
    for item in items.values() {
        names
            .entry((item.category, item.name.clone()))
            .or_default()
            .push(item.id.clone());
    }
    for item in items.values_mut() {
        item.provenance = summarize_provenance(item);
        item.candidate_ids = names
            .get(&(item.category, item.name.clone()))
            .into_iter()
            .flatten()
            .filter(|id| *id != &item.id && !library.relations.hides(&item.id, id))
            .cloned()
            .collect();
        item.separated_asset_ids = library.relations.separated(&item.id);
        item.manual_associations = super::relations::summaries(library, snapshot, &item.id);
    }
    for item in items.values_mut() {
        item.definition_removal = super::library_management::definition_removal(library, &item.id);
        super::usage::group_bindings(item, &observations);
        let agent_observations = native_observations.agents(item, &targets);
        item.application = application(
            &library.entries[&item.id],
            item,
            &observations,
            identity.conflicts.contains(&item.id),
            &targets,
            agent_observations,
        );
    }
    diagnostics.extend(super::relations::diagnostics(library).iter().cloned());
    if Instant::now() >= deadline {
        diagnostics
            .push("部分包比较达到有界预算；保留原生条目，不将未知当作相同或缺失。".to_owned());
    }
    if remaining_bytes == 0 {
        diagnostics.push(
            "完整定义比较达到 64 MiB 总预算；未完成来源保持未知，不据历史指纹归并。".to_owned(),
        );
    }
    library.entries.retain(|_, entry| {
        !entry.aliases.is_empty() || entry.current().is_some() || !entry.receipts.is_empty()
    });
    Ok(AgentAssetCatalog {
        revision: String::new(),
        counts: BTreeMap::new(),
        creatable_categories: vec![
            AgentAssetCategory::Skill,
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Hook,
        ],
        inventory: snapshot.inventory.clone(),
        assets: items.into_values().collect(),
        targets,
        diagnostics,
    })
}

fn summarize_provenance(asset: &AgentCatalogAsset) -> AgentAssetProvenanceSummary {
    let mut provisions = BTreeSet::new();
    let mut installations = BTreeSet::new();
    let mut providers = BTreeSet::new();
    for evidence in asset
        .bindings
        .iter()
        .flat_map(|binding| &binding.native.provenance)
    {
        provisions.insert(evidence.provision);
        installations.insert(evidence.installation);
        providers.insert(evidence.provider);
    }
    AgentAssetProvenanceSummary {
        provisions: provisions.into_iter().collect(),
        installations: installations.into_iter().collect(),
        providers: providers.into_iter().collect(),
    }
}

fn scoped_target_id(
    context: &AgentConfigurationContext,
    source: &AgentAssetSource,
    scope: AgentAssetScope,
) -> String {
    stable_id(
        "catalog-target",
        &[
            &context.id,
            &source.id,
            &format!("{scope:?}"),
            if source.source_kind == AgentAssetSourceKind::Directory {
                "skill"
            } else {
                "mcp"
            },
        ],
    )
}

pub(super) fn targets(inventory: &AgentEnvironmentInventory) -> Vec<AgentCatalogTarget> {
    let mut targets = Vec::new();
    for context in &inventory.contexts {
        let Some(adapter) = definition(context.agent_kind).environment.catalog_adapter() else {
            continue;
        };
        for source in adapter.target_sources(inventory, context) {
            for scope in adapter.target_scopes(source, context) {
                let category = if source.source_kind == AgentAssetSourceKind::Directory {
                    AgentAssetCategory::Skill
                } else {
                    AgentAssetCategory::Mcp
                };
                let mut reason = None;
                if !cfg!(unix) {
                    reason = Some("该平台的原子文件分发尚未验证".to_owned());
                }
                if adapter.requires_workspace_trust(category, scope)
                    && context.trust_context != AgentTrustState::Trusted
                {
                    reason = Some("请先在 Agent 原生工具中信任该项目".to_owned());
                }
                // Target layout, trust and writability determine candidacy.
                // The preview validates the live document and lossless native
                // conversion; historical CLI versions are not a write gate.
                targets.push(AgentCatalogTarget {
                    id: scoped_target_id(context, source, scope),
                    context_id: context.id.clone(),
                    agent_kind: context.agent_kind,
                    scope,
                    label: format!(
                        "{} · {} · {} · {}",
                        definition(context.agent_kind).label,
                        match scope {
                            AgentAssetScope::User => "用户",
                            AgentAssetScope::Local => "项目本地",
                            _ => "项目",
                        },
                        if category == AgentAssetCategory::Skill {
                            "Skill"
                        } else {
                            "MCP"
                        },
                        source.path,
                    ),
                    categories: vec![category],
                    available: reason.is_none(),
                    reason,
                });
            }
        }
    }
    targets.extend(super::hook_targets::targets(inventory));
    targets
}

pub(super) fn resolve_target<'a>(
    snapshot: &'a MutationInventory,
    id: &str,
) -> Result<
    (
        &'a AgentConfigurationContext,
        &'a AgentAssetSource,
        &'static NativeCatalogAdapter,
    ),
    String,
> {
    let (context, source, adapter) = observed_target(snapshot, id)?;
    if adapter
        .target_sources(&snapshot.inventory, context)
        .iter()
        .any(|candidate| candidate.id == source.id)
    {
        Ok((context, source, adapter))
    } else {
        Err("应用目标不存在或已离开当前配置上下文".to_owned())
    }
}

/// Display snapshots omit writable root authority. Resolve their published
/// IDs without mistaking missing guards for missing destinations. Writers use
/// resolve_target with a fresh inventory and reopen the physical guards.
pub(super) fn observed_target<'a>(
    snapshot: &'a MutationInventory,
    id: &str,
) -> Result<
    (
        &'a AgentConfigurationContext,
        &'a AgentAssetSource,
        &'static NativeCatalogAdapter,
    ),
    String,
> {
    for context in &snapshot.inventory.contexts {
        let Some(adapter) = definition(context.agent_kind).environment.catalog_adapter() else {
            continue;
        };
        for source in snapshot
            .inventory
            .sources
            .iter()
            .filter(|source| source.context_id == context.id)
        {
            if adapter
                .target_scopes(source, context)
                .into_iter()
                .any(|scope| scoped_target_id(context, source, scope) == id)
            {
                return Ok((context, source, adapter));
            }
        }
    }
    Err("应用目标不存在或已离开当前配置上下文".to_owned())
}

/// Observation paths support display grouping and preview scope only. Writers
/// must reopen and validate their physical anchors before preparing changes.
pub(super) fn source_path<'a>(
    snapshot: &'a MutationInventory,
    source: &'a AgentAssetSource,
) -> &'a std::path::Path {
    snapshot.source_anchors.get(&source.id).map_or_else(
        || std::path::Path::new(&source.path),
        |anchor| anchor.physical_path(),
    )
}

pub(super) fn bindings_at_target<'a>(
    snapshot: &MutationInventory,
    item: &'a AgentCatalogAsset,
    context_id: &str,
    scope: AgentAssetScope,
    source: &AgentAssetSource,
) -> Vec<&'a AgentCatalogBinding> {
    item.bindings
        .iter()
        .filter(|binding| {
            binding.native.context_id == context_id
                && binding.native.scope == scope
                && definition_source(snapshot, &binding.native).is_some_and(|origin| {
                    if item.category == AgentAssetCategory::Skill {
                        std::path::Path::new(&origin.path).starts_with(&source.path)
                    } else {
                        origin.id == source.id
                    }
                })
        })
        .collect()
}
