//! Current complete observations establish passive identity, never library
//! versions or native write authority. Historical aliases preserve identity
//! after drift, but historical fingerprints cannot admit a new binding.
use super::{
    definition::DefinitionPayload,
    opaque_id,
    repository::{self, Entry, Library},
};
use crate::models::*;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) const OWNERSHIP_CONFLICT: &str =
    "同一物理来源对应多份共享定义或恢复记录；已保留全部版本，请先处理归属冲突";

pub(super) struct CurrentDefinition {
    pub payload: Result<Arc<DefinitionPayload>, String>,
    pub fingerprint: Option<String>,
    pub physical_key: Option<String>,
    pub source_path: Option<String>,
    pub hook: Option<AgentCatalogHookRule>,
}

pub(super) struct Identity {
    pub bindings: BTreeMap<String, String>,
    pub conflicts: BTreeSet<String>,
}

/// Connected physical groups and already established aliases move together.
/// The representative below is only an internal grouping key, not a new public
/// ID; choosing the surviving existing library ID happens after conflict checks.
#[derive(Default)]
struct Groups {
    parents: BTreeMap<String, String>,
}

impl Groups {
    fn root(&self, id: &str) -> String {
        let mut current = id;
        while let Some(parent) = self.parents.get(current) {
            if parent == current {
                break;
            }
            current = parent;
        }
        current.to_owned()
    }

    fn join(&mut self, ids: &[String]) {
        let roots = ids.iter().map(|id| self.root(id)).collect::<BTreeSet<_>>();
        let Some(root) = roots.first() else {
            return;
        };
        for id in ids.iter().chain(roots.iter()) {
            self.parents.insert(id.clone(), root.clone());
        }
    }

    fn components(&self) -> Vec<Vec<String>> {
        let mut groups = BTreeMap::<String, Vec<String>>::new();
        for id in self.parents.keys() {
            groups.entry(self.root(id)).or_default().push(id.clone());
        }
        groups.into_values().collect()
    }
}

fn retained_owner(entry: &Entry) -> bool {
    !entry.versions.is_empty() || !entry.receipts.is_empty()
}

fn preferred_owner(
    library: &Library,
    existing: &BTreeSet<String>,
    ids: &[String],
) -> Result<String, String> {
    ids.iter()
        .min_by_key(|id| {
            let entry = &library.entries[*id];
            (
                Reverse(retained_owner(entry)),
                Reverse(existing.contains(*id)),
                Reverse(entry.aliases.len()),
                (*id).clone(),
            )
        })
        .cloned()
        .ok_or_else(|| "资产观察组不能为空".to_owned())
}

fn merge_component(library: &mut Library, owner: &str, members: &[String]) -> Result<(), String> {
    for id in members.iter().filter(|id| id.as_str() != owner) {
        let source = library.entries.get(id).ok_or("资产观察归属已变化")?;
        if retained_owner(source) {
            return Err("不能自动合并独立受管历史或恢复记录".to_owned());
        }
        let source = library.entries.remove(id).ok_or("资产观察归属已变化")?;
        let destination = library.entries.get_mut(owner).ok_or("资产观察归属已变化")?;
        destination.aliases.extend(source.aliases);
        for (fingerprint, variant_id) in source.variants {
            destination
                .variants
                .entry(fingerprint)
                .or_insert(variant_id);
        }
    }
    library.relations.remap(owner, members);
    Ok(())
}

pub(super) fn reconcile(
    library: &mut Library,
    assets: &[AgentAssetRecord],
    observations: &BTreeMap<String, CurrentDefinition>,
) -> Result<Identity, String> {
    let existing = library.entries.keys().cloned().collect::<BTreeSet<_>>();
    let mut bindings = BTreeMap::new();
    let mut physical = BTreeMap::<String, Vec<String>>::new();
    let mut groups = Groups::default();
    // Sorting also makes selection independent of adapter enumeration order.
    let mut current = assets.iter().collect::<Vec<_>>();
    current.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    for asset in &current {
        let observation = &observations[&asset.stable_id];
        let mut candidates = library
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.category == asset.category
                    && (entry.aliases.contains_key(&asset.stable_id)
                        || entry.receipts.values().any(|receipt| {
                            repository::receipt_matches(
                                receipt,
                                asset,
                                observation.source_path.as_deref(),
                            )
                        }))
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            let id = opaque_id()?;
            library
                .entries
                .insert(id.clone(), Entry::new(asset.label.clone(), asset.category));
            candidates.push(id);
        }
        let owner = preferred_owner(library, &existing, &candidates)?;
        bindings.insert(asset.stable_id.clone(), owner.clone());
        groups.join(&candidates);
        if let Some(key) = &observation.physical_key {
            physical.entry(key.clone()).or_default().push(owner);
        }
    }
    let mut physical_groups = Groups::default();
    for owners in physical.values() {
        physical_groups.join(owners);
        groups.join(owners);
    }
    for component in physical_groups.components() {
        library.relations.invalidate_physical(&component);
    }
    let mut conflicts = BTreeSet::new();
    for component in groups.components() {
        if library.relations.blocks_members(&component) {
            // Duplicated historical aliases without a verified current
            // physical anchor cannot override an explicit separation.
            conflicts.extend(component);
            continue;
        }
        if component
            .iter()
            .filter(|id| retained_owner(&library.entries[*id]))
            .count()
            > 1
        {
            // No history is discarded, including receipt-only recovery owners.
            // Unowned members of this physical group inherit its uncertainty.
            conflicts.extend(component);
            continue;
        }
        let owner = preferred_owner(library, &existing, &component)?;
        merge_component(library, &owner, &component)?;
        for id in bindings.values_mut() {
            if component.contains(id) {
                id.clone_from(&owner);
            }
        }
    }

    let mut members = BTreeMap::<String, Vec<&AgentAssetRecord>>::new();
    for asset in current {
        members
            .entry(bindings[&asset.stable_id].clone())
            .or_default()
            .push(asset);
    }
    let mut equivalent = BTreeMap::<(String, String, String), Vec<String>>::new();
    for (owner, members) in members {
        if conflicts.contains(&owner)
            || !matches!(
                library.entries[&owner].category,
                AgentAssetCategory::Skill | AgentAssetCategory::Mcp | AgentAssetCategory::Hook
            )
        {
            continue;
        }
        let Some(fingerprint) = observations[&members[0].stable_id].fingerprint.as_deref() else {
            continue;
        };
        // A partly observed or already divergent logical asset cannot provide
        // an unambiguous equality witness for admitting an unrelated source.
        if members.iter().any(|asset| {
            let observation = &observations[&asset.stable_id];
            (if asset.category == AgentAssetCategory::Hook {
                observation.hook.is_none()
            } else {
                observation.payload.is_err()
            }) || observation.physical_key.is_none()
                || observation.fingerprint.as_deref() != Some(fingerprint)
        }) {
            continue;
        }
        for asset in members {
            equivalent
                .entry((
                    asset.category.key().to_owned(),
                    if asset.category == AgentAssetCategory::Hook {
                        // The complete rule includes its event and group metadata.
                        // Plugin/source identity remains on each native binding.
                        String::new()
                    } else {
                        asset.native_id.clone()
                    },
                    fingerprint.to_owned(),
                ))
                .or_default()
                .push(owner.clone());
        }
    }
    let mut groups = Groups::default();
    let mut edges = BTreeSet::new();
    for owners in equivalent.values() {
        for owner in owners {
            groups.join(std::slice::from_ref(owner));
        }
        for left in owners {
            for right in owners {
                if left < right {
                    edges.insert((left.clone(), right.clone()));
                }
            }
        }
    }
    for (left, right) in edges {
        let left_root = groups.root(&left);
        let right_root = groups.root(&right);
        if left_root == right_root {
            continue;
        }
        let proposed = groups
            .parents
            .keys()
            .filter(|id| {
                let root = groups.root(id);
                root == left_root || root == right_root
            })
            .cloned()
            .collect::<Vec<_>>();
        // Check complete components, so A separated from B cannot be joined
        // indirectly through a third equal definition C.
        if !library.relations.blocks_members(&proposed) {
            groups.join(&[left, right]);
        }
    }
    for component in groups.components() {
        if component
            .iter()
            .filter(|id| retained_owner(&library.entries[*id]))
            .count()
            > 1
        {
            // Equal native content does not authorize merging two independent
            // version/receipt histories. They remain explicit candidates.
            continue;
        }
        let owner = preferred_owner(library, &existing, &component)?;
        merge_component(library, &owner, &component)?;
        for id in bindings.values_mut() {
            if component.contains(id) {
                id.clone_from(&owner);
            }
        }
    }
    Ok(Identity {
        bindings,
        conflicts,
    })
}

/// Select a source without exposing private equality evidence through IPC.
pub(super) fn application_source<'a>(
    entry: &'a Entry,
    asset: &AgentCatalogAsset,
    observations: &'a BTreeMap<String, CurrentDefinition>,
    conflicted: bool,
) -> Result<(&'a DefinitionPayload, Option<String>), String> {
    if conflicted {
        return Err(OWNERSHIP_CONFLICT.to_owned());
    }
    if entry.receipts.values().any(|receipt| {
        receipt
            .hook
            .as_ref()
            .is_some_and(|hook| hook.pending.is_some())
    }) {
        return Err("上次 Hook 提交尚未完成对账，当前归属不能用于分发".to_owned());
    }
    if let Some(current) = entry.current() {
        return Ok((&current.payload, None));
    }
    let source = asset
        .bindings
        .iter()
        .min_by(|left, right| left.id.cmp(&right.id))
        .ok_or("当前没有可完整读取的原生定义，请先检查来源")?;
    let observation = &observations[&source.id];
    let payload = observation.payload.as_ref().map_err(Clone::clone)?;
    let fingerprint = observation
        .fingerprint
        .as_deref()
        .ok_or("当前定义缺少完整比较证据")?;
    for binding in &asset.bindings {
        if !binding.can_adopt {
            return Err(binding
                .reason
                .clone()
                .unwrap_or_else(|| "当前定义不完整或不可独立收录".to_owned()));
        }
        let current = &observations[&binding.id];
        current.payload.as_ref().map_err(Clone::clone)?;
        if current.fingerprint.as_deref() != Some(fingerprint)
            || (asset.category == AgentAssetCategory::Hook
                && binding.native.agent_kind != source.native.agent_kind)
        {
            return Err("该资产有多个不同定义，请在详情中明确选择收录来源".to_owned());
        }
    }
    Ok((payload, Some(source.id.clone())))
}
