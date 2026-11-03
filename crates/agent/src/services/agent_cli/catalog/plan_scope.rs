//! Bound a preview to its explicit resource, candidate conflicts and shared
//! physical consumers. The original fresh inventory remains the write authority.
use super::{projection, repository::Library, PublishedCatalog};
use crate::{models::*, services::agent_cli::environment::mutation::MutationInventory};
use std::collections::BTreeSet;

fn source_id(request: &AgentCatalogPlanRequest) -> Option<&str> {
    match &request.source {
        AgentCatalogPlanSource::Catalog { asset_id, .. }
        | AgentCatalogPlanSource::NativeBinding { asset_id, .. } => Some(asset_id),
        AgentCatalogPlanSource::Draft { definition } => definition.asset_id.as_deref(),
    }
}

pub(crate) fn agents(
    publication: &PublishedCatalog,
    request: &AgentCatalogPlanRequest,
) -> Result<BTreeSet<AgentCliKind>, String> {
    let catalog = &publication.catalog;
    let snapshot = &publication.snapshot;
    let item = source_id(request).and_then(|id| catalog.assets.iter().find(|item| item.id == id));
    let mut kinds = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut bindings = Vec::new();
    for id in &request.target_ids {
        if let Some(target) = catalog.targets.iter().find(|target| target.id == *id) {
            kinds.insert(target.agent_kind);
            if let Ok((_, source, _)) = projection::observed_target(snapshot, id) {
                source_ids.insert(source.id.as_str());
            } else if let Ok(target) = super::hook_targets::resolve(snapshot, id, None) {
                source_ids.insert(target.source.id.as_str());
            }
        } else if let Some(binding) =
            item.and_then(|item| item.bindings.iter().find(|binding| binding.id == *id))
        {
            bindings.push(binding);
        } else if let Some(target) = item.and_then(|item| {
            item.unresolved_targets
                .iter()
                .find(|target| target.target_id == *id)
        }) {
            kinds.insert(target.agent_kind);
        } else {
            return Err("所选目标已变化，请重新选择".to_owned());
        }
    }
    if let AgentCatalogPlanSource::NativeBinding { binding_id, .. } = &request.source {
        bindings.push(
            item.and_then(|item| {
                item.bindings
                    .iter()
                    .find(|binding| binding.id == *binding_id)
            })
            .ok_or("原生来源已变化")?,
        );
    }
    for binding in bindings {
        kinds.insert(binding.native.agent_kind);
        source_ids.extend(binding.native.source_ids.iter().map(String::as_str));
        source_ids.insert(binding.native.inspection_source_id.as_str());
        // Parent operations can affect other members. Use the published graph
        // for scope only; fresh preparation still verifies the actual owner.
        for owner in snapshot.inventory.assets.iter().filter(|asset| {
            binding.native.relationships.action_owner.as_deref() == Some(asset.stable_id.as_str())
                || binding.native.relationships.provided_by.as_deref()
                    == Some(asset.stable_id.as_str())
        }) {
            kinds.insert(owner.agent_kind);
            source_ids.extend(owner.source_ids.iter().map(String::as_str));
            source_ids.insert(owner.inspection_source_id.as_str());
            kinds.extend(
                snapshot
                    .inventory
                    .assets
                    .iter()
                    .filter(|asset| {
                        owner
                            .relationships
                            .affected_asset_ids
                            .contains(&asset.stable_id)
                    })
                    .map(|asset| asset.agent_kind),
            );
        }
    }
    let selected_sources = snapshot
        .inventory
        .sources
        .iter()
        .filter(|source| source_ids.contains(source.id.as_str()))
        .collect::<Vec<_>>();
    for source in &snapshot.inventory.sources {
        let matches = selected_sources.iter().any(|selected| {
            let path = projection::source_path(snapshot, source);
            let selected_path = projection::source_path(snapshot, selected);
            if path.starts_with(selected_path) || selected_path.starts_with(path) {
                return true;
            }
            // A persisted publication has no physical anchors. Shared/linked
            // locations can be aliases, so include possible consumers until
            // the scoped inventory resolves their current physical paths.
            let uncertain = !snapshot.source_anchors.contains_key(&selected.id)
                || !snapshot.source_anchors.contains_key(&source.id);
            uncertain
                && source.scope == selected.scope
                && selected
                    .categories
                    .iter()
                    .any(|category| source.categories.contains(category))
                && (source.origin == AgentAssetInstallationOrigin::SharedFiles
                    || selected.origin == AgentAssetInstallationOrigin::SharedFiles
                    || source.origin == AgentAssetInstallationOrigin::Linked
                    || selected.origin == AgentAssetInstallationOrigin::Linked
                    || source.revision.is_symlink
                    || selected.revision.is_symlink)
        });
        if matches {
            if let Some(context) = snapshot
                .inventory
                .contexts
                .iter()
                .find(|context| context.id == source.context_id)
            {
                kinds.insert(context.agent_kind);
            }
        }
    }
    if kinds.is_empty() {
        return Err("尚未选择预览目标".to_owned());
    }
    Ok(kinds)
}

pub(super) fn focus(
    snapshot: &MutationInventory,
    catalog: &AgentAssetCatalog,
    request: &AgentCatalogPlanRequest,
    library: &mut Library,
) -> Result<MutationInventory, String> {
    let item = source_id(request)
        .map(|id| {
            catalog
                .assets
                .iter()
                .find(|item| item.id == id)
                .ok_or("资产已离开目录")
        })
        .transpose()?;
    let category = item
        .map(|item| item.category)
        .or(match &request.source {
            AgentCatalogPlanSource::Draft { definition } => Some(definition.category),
            _ => None,
        })
        .ok_or("资产类别无效")?;
    let mut names = BTreeSet::new();
    if let Some(item) = item {
        names.insert(item.name.as_str());
    }
    if let AgentCatalogPlanSource::Draft { definition } = &request.source {
        names.insert(definition.name.as_str());
    }
    let known = item
        .into_iter()
        .flat_map(|item| &item.bindings)
        .map(|binding| binding.id.as_str())
        .collect::<BTreeSet<_>>();
    let physical = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| known.contains(asset.stable_id.as_str()))
        .filter_map(|asset| projection::definition_source(snapshot, asset))
        .filter_map(|source| snapshot.source_anchors.get(&source.id))
        .map(|anchor| anchor.physical_path())
        .collect::<BTreeSet<_>>();
    let mut focused = MutationInventory {
        inventory: snapshot.inventory.clone(),
        source_anchors: snapshot.source_anchors.clone(),
    };
    focused.inventory.assets.retain(|asset| {
        asset.category == category
            && (known.contains(asset.stable_id.as_str())
                || names.contains(asset.native_id.as_str())
                || names.contains(asset.label.as_str())
                || projection::definition_source(snapshot, asset)
                    .and_then(|source| snapshot.source_anchors.get(&source.id))
                    .is_some_and(|anchor| physical.contains(anchor.physical_path())))
    });
    let relevant = focused
        .inventory
        .assets
        .iter()
        .map(|asset| asset.stable_id.as_str())
        .collect::<BTreeSet<_>>();
    library.entries.retain(|id, entry| {
        source_id(request) == Some(id.as_str())
            || (entry.category == category
                && (names.contains(entry.name.as_str())
                    || entry
                        .aliases
                        .keys()
                        .any(|id| relevant.contains(id.as_str()))
                    || entry.receipts.values().any(|receipt| {
                        focused.inventory.assets.iter().any(|asset| {
                            super::repository::receipt_matches(
                                receipt,
                                asset,
                                projection::definition_source(&focused, asset)
                                    .map(|source| source.path.as_str()),
                            )
                        })
                    })))
    });
    Ok(focused)
}
