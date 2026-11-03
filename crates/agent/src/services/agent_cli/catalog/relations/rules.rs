//! One rule source for relation capabilities, confirmation and catalog hints.
use super::{pair_key, AssociationOrigin, ManualAssociation};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::{
            digest,
            hook_receipts::HookBindingState,
            opaque_id, projection,
            repository::{self, Entry, Library},
        },
        environment::mutation::{GuardedFile, MutationInventory},
    },
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub(super) struct Change {
    pub asset_ids: BTreeSet<String>,
    pub binding_ids: BTreeSet<String>,
    pub receipt_ids: BTreeSet<String>,
    pub domains: Vec<String>,
    moves: BTreeMap<String, (String, String)>,
    receipt_moves: BTreeMap<String, (String, String)>,
    receipt_signatures: BTreeMap<String, String>,
    seeds: BTreeMap<String, Entry>,
    required_existing: BTreeSet<String>,
}

impl Change {
    pub(super) fn signature(&self) -> Result<String, String> {
        serde_json::to_vec(self)
            .map(|bytes| digest(&bytes))
            .map_err(|_| "关系计划无法编码".to_owned())
    }
    pub(super) fn estimate_bytes(&self) -> usize {
        serde_json::to_vec(self).map_or(usize::MAX, |bytes| bytes.len())
    }
    pub(super) fn bindings_moving_to(&self, asset_id: &str) -> BTreeSet<String> {
        self.moves
            .iter()
            .filter(|(_, (_, destination))| destination == asset_id)
            .map(|(binding_id, _)| binding_id.clone())
            .collect()
    }
    pub(super) fn seed_missing(&self, library: &mut Library) -> Result<(), String> {
        for (id, seed) in &self.seeds {
            if !library.entries.contains_key(id) {
                if self.required_existing.contains(id) {
                    return Err("关系来源的受管记录已缺失".to_owned());
                }
                library.entries.insert(id.clone(), seed.clone());
            }
        }
        Ok(())
    }
}

pub(super) fn prepare(
    library: &Library,
    snapshot: &MutationInventory,
    intent: &AgentCatalogRelationIntent,
) -> Result<Change, String> {
    let mut change = Change {
        asset_ids: BTreeSet::new(),
        binding_ids: BTreeSet::new(),
        receipt_ids: BTreeSet::new(),
        domains: Vec::new(),
        moves: BTreeMap::new(),
        receipt_moves: BTreeMap::new(),
        receipt_signatures: BTreeMap::new(),
        seeds: BTreeMap::new(),
        required_existing: BTreeSet::new(),
    };
    match intent {
        AgentCatalogRelationIntent::Merge {
            destination_asset_id,
            source_asset_id,
        } => {
            let (destination, source) = pair(library, destination_asset_id, source_asset_id)?;
            if source.current().is_some() {
                return Err(
                    "来源已有独立共享版本；不能合并或丢弃其版本历史，可选择相反方向".to_owned(),
                );
            }
            if source.aliases.is_empty() {
                return Err("来源没有当前原生绑定可供合并".to_owned());
            }
            if library.relations.associations.len() >= 4096 {
                return Err("手工对应记录已达到有界上限".to_owned());
            }
            reject_pending(destination)?;
            reject_pending(source)?;
            let source_keys = verified_entry(snapshot, source)?;
            verified_entry(snapshot, destination)?;
            // Every physical member is already owned by one of these entries.
            // An unrelated retained owner cannot be silently folded into a pair.
            let selected_keys = source_keys.values().collect::<BTreeSet<_>>();
            for (id, entry) in &library.entries {
                if id == source_asset_id || id == destination_asset_id {
                    continue;
                }
                if entry
                    .aliases
                    .keys()
                    .filter_map(|id| physical(snapshot, id).ok())
                    .any(|key| selected_keys.contains(&key))
                {
                    return Err("选中来源与第三项资产共用物理组，不能仅合并其中一部分".to_owned());
                }
            }
            if library.relations.associations.values().any(|record| {
                record.destination_asset_id == *source_asset_id
                    || record.sources.values().any(|origin| {
                        origin
                            .binding_ids
                            .iter()
                            .any(|id| source.aliases.contains_key(id))
                    })
            }) {
                return Err("来源包含另一条手工对应关系，请先处理该关系后再合并".to_owned());
            }
            for id in source.aliases.keys() {
                change.moves.insert(
                    id.clone(),
                    (source_asset_id.clone(), destination_asset_id.clone()),
                );
            }
            for (key, receipt) in &source.receipts {
                require_movable_receipt(
                    snapshot,
                    receipt,
                    &source.aliases.keys().cloned().collect(),
                )?;
                if destination.receipts.contains_key(key) {
                    return Err("目标已有同标识恢复记录，不能覆盖其归属".to_owned());
                }
                change.receipt_moves.insert(
                    key.clone(),
                    (source_asset_id.clone(), destination_asset_id.clone()),
                );
                change
                    .receipt_signatures
                    .insert(key.clone(), receipt_signature(receipt)?);
            }
            add_entry(&mut change, destination_asset_id, destination);
            add_entry(&mut change, source_asset_id, source);
        }
        AgentCatalogRelationIntent::KeepSeparate {
            left_asset_id,
            right_asset_id,
            ..
        } => {
            let (left, right) = pair(library, left_asset_id, right_asset_id)?;
            let left_keys = left
                .aliases
                .keys()
                .filter_map(|id| physical(snapshot, id).ok())
                .collect::<BTreeSet<_>>();
            if right
                .aliases
                .keys()
                .filter_map(|id| physical(snapshot, id).ok())
                .any(|key| left_keys.contains(&key))
            {
                return Err("两项共用同一物理来源，不能把真实共享文件标记为独立来源".to_owned());
            }
            if library.relations.separations.len() >= 8192
                && !library
                    .relations
                    .separations
                    .contains_key(&pair_key(left_asset_id, right_asset_id))
            {
                return Err("分开决定已达到有界上限".to_owned());
            }
            add_entry(&mut change, left_asset_id, left);
            add_entry(&mut change, right_asset_id, right);
        }
        AgentCatalogRelationIntent::RestoreHint {
            left_asset_id,
            right_asset_id,
        } => {
            let (left, right) = pair(library, left_asset_id, right_asset_id)?;
            if !library
                .relations
                .separations
                .get(&pair_key(left_asset_id, right_asset_id))
                .is_some_and(|pair| pair.hide_candidate)
            {
                return Err("这两项当前没有隐藏的同名提醒".to_owned());
            }
            add_entry(&mut change, left_asset_id, left);
            add_entry(&mut change, right_asset_id, right);
        }
        AgentCatalogRelationIntent::Detach { association_id } => {
            prepare_detach(library, snapshot, association_id, &mut change)?
        }
        AgentCatalogRelationIntent::Compare { .. } => {
            return Err("纯比较没有可提交的关系变更".to_owned())
        }
    }
    change.binding_ids.extend(change.moves.keys().cloned());
    change
        .receipt_ids
        .extend(change.receipt_moves.keys().cloned());
    if change.binding_ids.len() > 64 {
        return Err("本次关系涉及超过 64 个绑定，请缩小整理范围".to_owned());
    }
    let all_bindings = change
        .seeds
        .values()
        .flat_map(|entry| entry.aliases.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    change.domains = domains(snapshot, &all_bindings);
    Ok(change)
}

fn pair<'a>(
    library: &'a Library,
    left: &str,
    right: &str,
) -> Result<(&'a Entry, &'a Entry), String> {
    if left == right {
        return Err("请选择两项不同资产".to_owned());
    }
    let left = library.entries.get(left).ok_or("左侧资产已不存在")?;
    let right = library.entries.get(right).ok_or("右侧资产已不存在")?;
    if left.category != right.category {
        return Err("只能整理相同类别的资产关系".to_owned());
    }
    Ok((left, right))
}

fn add_entry(change: &mut Change, id: &str, entry: &Entry) {
    change.asset_ids.insert(id.to_owned());
    if entry.current().is_some() || !entry.receipts.is_empty() {
        change.required_existing.insert(id.to_owned());
    }
    let mut seed = Entry::new(entry.name.clone(), entry.category);
    seed.aliases = entry.aliases.clone();
    seed.variants = entry.variants.clone();
    change.seeds.insert(id.to_owned(), seed);
}

fn physical(snapshot: &MutationInventory, id: &str) -> Result<String, String> {
    let asset = snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == id)
        .ok_or("部分来源不在当前盘点，请切换到对应工作目录后刷新")?;
    crate::services::agent_cli::catalog::observation::DefinitionReader::new(snapshot)
        .physical_key(asset)
        .ok_or_else(|| "来源路径或完整物理归属无法验证，请先检查来源".to_owned())
}

fn verified_entry(
    snapshot: &MutationInventory,
    entry: &Entry,
) -> Result<BTreeMap<String, String>, String> {
    entry
        .aliases
        .iter()
        .map(|(id, observation)| {
            let current = physical(snapshot, id)?;
            if observation.physical_key.as_deref() != Some(current.as_str()) {
                return Err("来源的物理身份已变化，请先刷新完整目录".to_owned());
            }
            Ok((id.clone(), current))
        })
        .collect()
}

fn reject_pending(entry: &Entry) -> Result<(), String> {
    if entry.receipts.values().any(|receipt| {
        receipt
            .hook
            .as_ref()
            .is_some_and(|hook| hook.pending.is_some())
    }) {
        return Err("Hook 尚未完成提交对账，恢复记录已保留，当前不能整理归属".to_owned());
    }
    Ok(())
}

fn require_movable_receipt(
    snapshot: &MutationInventory,
    receipt: &repository::Receipt,
    ids: &BTreeSet<String>,
) -> Result<(), String> {
    if receipt.version != 0
        || !receipt
            .hook
            .as_ref()
            .is_some_and(|hook| hook.state == HookBindingState::Active && hook.pending.is_none())
    {
        return Err("来源含受管应用历史或暂停恢复记录，不能把它移到另一逻辑资产".to_owned());
    }
    if !snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| ids.contains(&asset.stable_id))
        .any(|asset| {
            repository::receipt_matches(
                receipt,
                asset,
                projection::definition_source(snapshot, asset).map(|source| source.path.as_str()),
            )
        })
    {
        return Err("恢复记录不能精确对应当前完整来源，未改变归属".to_owned());
    }
    Ok(())
}

fn receipt_signature(receipt: &repository::Receipt) -> Result<String, String> {
    serde_json::to_vec(receipt)
        .map(|bytes| digest(&bytes))
        .map_err(|_| "恢复记录无法编码".to_owned())
}

fn domains(snapshot: &MutationInventory, ids: &BTreeSet<String>) -> Vec<String> {
    let mut result = BTreeSet::new();
    for asset in snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| ids.contains(&asset.stable_id))
    {
        if let Some(context) = snapshot
            .inventory
            .contexts
            .iter()
            .find(|context| context.id == asset.context_id)
        {
            result.insert(format!("config:{}", context.config_root));
        }
        if let Some(source) = projection::definition_source(snapshot, asset) {
            if let Ok(file) = GuardedFile::capture(source, &snapshot.source_anchors) {
                result.extend(file.lock_domains());
            }
        }
        if let Some(installation) = &asset.selected_action_installation_id {
            result.insert(format!("installation:{installation}"));
        }
    }
    result.into_iter().collect()
}

fn prepare_detach(
    library: &Library,
    snapshot: &MutationInventory,
    id: &str,
    change: &mut Change,
) -> Result<(), String> {
    let record = library
        .relations
        .associations
        .get(id)
        .ok_or("这条手工对应关系已不存在")?;
    let destination = library
        .entries
        .get(&record.destination_asset_id)
        .ok_or("对应关系的当前资产已不存在")?;
    reject_pending(destination)?;
    let current = verified_entry(snapshot, destination)?;
    let mut selected = BTreeMap::<String, String>::new();
    for (source_id, origin) in &record.sources {
        if origin.category != destination.category {
            return Err("手工对应记录的类别已变化".to_owned());
        }
        if library.entries.get(source_id).is_some_and(|entry| {
            !entry.aliases.is_empty() || entry.current().is_some() || !entry.receipts.is_empty()
        }) {
            return Err("原始逻辑标识已有新的独立归属，不能覆盖该记录".to_owned());
        }
        change.asset_ids.insert(source_id.clone());
        for binding in &origin.binding_ids {
            let key = current
                .get(binding)
                .ok_or("手工合并的原始来源已缺失或离开当前目录，无法安全解除")?;
            if selected
                .insert(key.clone(), source_id.clone())
                .is_some_and(|previous| previous != *source_id)
            {
                return Err("不同原始来源现已共享同一物理组，不能拆开".to_owned());
            }
        }
    }
    if record
        .destination_binding_ids
        .iter()
        .filter_map(|id| current.get(id))
        .any(|key| selected.contains_key(key))
    {
        return Err("合并前分属两侧的来源现已共享物理文件，不能解除成独立可写资产".to_owned());
    }
    for (binding, key) in current {
        if let Some(source) = selected.get(&key) {
            change.moves.insert(
                binding,
                (record.destination_asset_id.clone(), source.clone()),
            );
        }
    }
    if change.moves.is_empty() {
        return Err("手工对应的来源已不在当前目录".to_owned());
    }
    if library
        .relations
        .associations
        .iter()
        .filter(|(other_id, _)| other_id.as_str() != id)
        .any(|(_, other)| {
            other.sources.values().any(|origin| {
                origin
                    .binding_ids
                    .iter()
                    .any(|binding| change.moves.contains_key(binding))
            })
        })
    {
        return Err("当前来源同时参与另一条手工对应，不能单独解除".to_owned());
    }
    let moving = change.moves.keys().cloned().collect::<BTreeSet<_>>();
    for (key, receipt) in &destination.receipts {
        let owner = snapshot.inventory.assets.iter().find(|asset| {
            moving.contains(&asset.stable_id)
                && repository::receipt_matches(
                    receipt,
                    asset,
                    projection::definition_source(snapshot, asset)
                        .map(|source| source.path.as_str()),
                )
        });
        let Some(owner) = owner else {
            continue;
        };
        require_movable_receipt(snapshot, receipt, &moving)?;
        let source = &change.moves[&owner.stable_id].1;
        if library
            .entries
            .get(source)
            .is_some_and(|entry| entry.receipts.contains_key(key))
        {
            return Err("原始资产已有同标识恢复记录".to_owned());
        }
        change.receipt_moves.insert(
            key.clone(),
            (record.destination_asset_id.clone(), source.clone()),
        );
        change
            .receipt_signatures
            .insert(key.clone(), receipt_signature(receipt)?);
    }
    add_entry(change, &record.destination_asset_id, destination);
    Ok(())
}

pub(super) fn apply(
    library: &mut Library,
    snapshot: &MutationInventory,
    intent: &AgentCatalogRelationIntent,
    change: &Change,
) -> Result<AgentCatalogRelationCommitResult, String> {
    let mut association_id = None;
    let message = match intent {
        AgentCatalogRelationIntent::Merge {
            destination_asset_id,
            source_asset_id,
        } => {
            let source = library.entries.get(source_asset_id).ok_or("来源已不存在")?;
            let origin = AssociationOrigin {
                name: source.name.clone(),
                category: source.category,
                binding_ids: change.moves.keys().cloned().collect(),
            };
            let record = ManualAssociation {
                destination_asset_id: destination_asset_id.clone(),
                destination_binding_ids: library
                    .entries
                    .get(destination_asset_id)
                    .ok_or("目标已不存在")?
                    .aliases
                    .keys()
                    .cloned()
                    .collect(),
                sources: BTreeMap::from([(source_asset_id.clone(), origin)]),
            };
            move_ownership(library, change)?;
            library.relations.remap(
                destination_asset_id,
                &[source_asset_id.clone(), destination_asset_id.clone()],
            );
            let id = opaque_id()?;
            library.relations.associations.insert(id.clone(), record);
            association_id = Some(id);
            "已合并逻辑归属；原生文件、启用状态和共享版本均保留"
        }
        AgentCatalogRelationIntent::KeepSeparate {
            left_asset_id,
            right_asset_id,
            hide_candidate,
        } => {
            library
                .relations
                .separate(left_asset_id, right_asset_id, *hide_candidate);
            if *hide_candidate {
                "已保留分开并隐藏同名提醒；自动内容比较不会重新合并"
            } else {
                "已保留分开；同名比较提醒继续显示"
            }
        }
        AgentCatalogRelationIntent::RestoreHint {
            left_asset_id,
            right_asset_id,
        } => {
            library
                .relations
                .separations
                .get_mut(&pair_key(left_asset_id, right_asset_id))
                .ok_or("分开记录已不存在")?
                .hide_candidate = false;
            "已恢复同名提醒；保留分开的决定仍然有效"
        }
        AgentCatalogRelationIntent::Detach { association_id: id } => {
            let record = library
                .relations
                .associations
                .get(id)
                .cloned()
                .ok_or("手工对应已不存在")?;
            for (source_id, source) in &record.sources {
                library
                    .entries
                    .entry(source_id.clone())
                    .or_insert_with(|| Entry::new(source.name.clone(), source.category));
            }
            move_ownership(library, change)?;
            library.relations.associations.remove(id);
            let peers = library.relations.separated(&record.destination_asset_id);
            for source_id in record.sources.keys() {
                for peer in &peers {
                    if peer != source_id {
                        library.relations.separate(source_id, peer, false);
                    }
                }
                library
                    .relations
                    .separate(&record.destination_asset_id, source_id, false);
            }
            "已解除手工对应并保留分开；使用当前来源，不恢复或覆盖历史文件"
        }
        AgentCatalogRelationIntent::Compare { .. } => return Err("比较不能提交".to_owned()),
    };
    // Reopen the same current anchors at the final ownership boundary. The
    // operation has held every known shared native domain throughout the CAS.
    for id in &change.binding_ids {
        physical(snapshot, id)?;
    }
    library.entries.retain(|_, entry| {
        !entry.aliases.is_empty() || entry.current().is_some() || !entry.receipts.is_empty()
    });
    Ok(AgentCatalogRelationCommitResult {
        asset_ids: change.asset_ids.iter().cloned().collect(),
        association_id,
        message: message.to_owned(),
    })
}

fn move_ownership(library: &mut Library, change: &Change) -> Result<(), String> {
    for (id, (source_id, destination_id)) in &change.moves {
        let source = library.entries.get_mut(source_id).ok_or("来源已不存在")?;
        let observation = source.aliases.remove(id).ok_or("来源绑定已变化")?;
        let variant = observation.fingerprint.as_ref().and_then(|fingerprint| {
            source
                .variants
                .get(fingerprint)
                .map(|variant| (fingerprint.clone(), variant.clone()))
        });
        let destination = library
            .entries
            .get_mut(destination_id)
            .ok_or("目标已不存在")?;
        if destination
            .aliases
            .insert(id.clone(), observation)
            .is_some()
        {
            return Err("目标已有同标识绑定，未覆盖归属".to_owned());
        }
        if let Some((fingerprint, variant)) = variant {
            destination.variants.entry(fingerprint).or_insert(variant);
        }
    }
    for (id, (source_id, destination_id)) in &change.receipt_moves {
        let receipt = library
            .entries
            .get_mut(source_id)
            .ok_or("恢复记录来源已不存在")?
            .receipts
            .remove(id)
            .ok_or("恢复记录已变化")?;
        if library
            .entries
            .get_mut(destination_id)
            .ok_or("恢复记录目标已不存在")?
            .receipts
            .insert(id.clone(), receipt)
            .is_some()
        {
            return Err("目标已有恢复记录，未覆盖".to_owned());
        }
    }
    Ok(())
}
