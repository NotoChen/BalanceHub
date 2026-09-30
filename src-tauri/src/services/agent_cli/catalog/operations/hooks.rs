//! Canonical Hook selection and adapter-owned same-document edit composition.
use super::super::{
    definition::{DefinitionPayload, StoredDefinition},
    digest, hook_definition,
    hook_receipts::{HookBindingState, HookFileEvidence, HookReceipt},
    hook_targets,
    native::hooks::*,
    repository::{Entry, Receipt},
    CatalogService,
};
use super::hook_guard::HookReadGuard;
use crate::{
    models::*,
    services::{
        agent_cli::{
            definition,
            environment::{
                mutation::{GuardedFile, MutationInspector, MutationInventory},
                stable_id,
            },
        },
        agent_runtime::managed_hook,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct ReceiptTransition {
    pub key: String,
    pub before: Option<Receipt>,
    pub after: Option<Receipt>,
    pub carrier: Receipt,
}

pub(super) struct HookMember {
    pub index: usize,
    pub target_id: String,
    pub label: String,
    pub context: AgentConfigurationContext,
    pub destination: HookNativeDestination,
    pub adapter: &'static NativeHookAdapter,
    pub edit: Option<HookNativeEdit>,
    pub transition: Option<ReceiptTransition>,
    write_source_ids: BTreeSet<String>,
    policy_expectations: Vec<HookNativeExpectation>,
    pub changes: Vec<AgentAssetPlanChange>,
    pub signature: String,
}

pub(super) struct HookWrite {
    pub indexes: Vec<usize>,
    pub file: GuardedFile,
    pub bytes: Vec<u8>,
    pub root: PathBuf,
    pub evidence_root: String,
    pub source_id: Option<String>,
}

pub(super) struct PreparedHookGroup {
    pub members: Vec<HookMember>,
    pub reads: BTreeMap<String, HookReadGuard>,
    pub writes: Vec<HookWrite>,
    pub expectations: Vec<(AgentCliKind, HookNativeExpectation)>,
    pub affected_asset_ids: Vec<String>,
    pub notes: Vec<String>,
    pub domains: Vec<String>,
    pub signature: String,
}

impl PreparedHookGroup {
    pub(super) fn retained_private_bytes(&self) -> usize {
        let reads = self.reads.values().fold(0_usize, |total, read| {
            total.saturating_add(read.retained_private_bytes())
        });
        let writes = self.writes.iter().fold(0_usize, |total, write| {
            total
                .saturating_add(write.file.bytes().map_or(0, <[u8]>::len))
                .saturating_add(write.bytes.len())
        });
        let members = self.members.iter().fold(0_usize, |total, member| {
            total.saturating_add(member.retained_private_bytes())
        });
        let expectations = self
            .expectations
            .iter()
            .fold(0_usize, |total, (_, expectation)| {
                total.saturating_add(super::cache::encoded_bytes(&expectation.definition))
            });
        reads
            .saturating_add(writes)
            .saturating_add(members)
            .saturating_add(expectations)
    }
}

impl HookMember {
    fn retained_private_bytes(&self) -> usize {
        let edit = self.edit.as_ref().map_or(0, |edit| match edit {
            HookNativeEdit::Add { definition } => super::cache::encoded_bytes(definition),
            HookNativeEdit::Replace {
                original,
                definition,
            }
            | HookNativeEdit::Restore {
                original,
                definition,
            } => super::cache::encoded_bytes(&(original, definition)),
            HookNativeEdit::Remove { original } | HookNativeEdit::SetEnabled { original, .. } => {
                super::cache::encoded_bytes(original)
            }
        });
        let transition = self.transition.as_ref().map_or(0, |transition| {
            super::cache::encoded_bytes(&(
                &transition.before,
                &transition.after,
                &transition.carrier,
            ))
        });
        let expectations = self
            .policy_expectations
            .iter()
            .fold(0_usize, |total, expectation| {
                total.saturating_add(super::cache::encoded_bytes(&expectation.definition))
            });
        edit.saturating_add(transition)
            .saturating_add(expectations)
            .saturating_add(super::cache::encoded_bytes(&(
                &self.context,
                &self.destination,
                &self.changes,
            )))
    }

    pub fn prepare(
        snapshot: &MutationInventory,
        item: &AgentCatalogAsset,
        entry: &Entry,
        target_id: &str,
        action: AgentCatalogAction,
        index: usize,
    ) -> Result<Self, String> {
        let is_apply = action == AgentCatalogAction::ApplyDefinition;
        if is_apply && (item.version.is_none() || item.application.source_binding_id.is_some()) {
            return Err("请先收录当前原生 Hook，再选择已保存版本进行应用".to_owned());
        }
        if is_apply && !item.application.available {
            return Err(item
                .application
                .reason
                .clone()
                .unwrap_or_else(|| "共享 Hook 定义当前不可应用".to_owned()));
        }
        let direct_receipt = entry.receipts.get(target_id);
        if direct_receipt
            .and_then(|receipt| receipt.hook.as_ref())
            .is_some_and(|hook| hook.pending.is_some())
        {
            return Err("上次 Hook 提交尚未完成对账，不能重复写入".to_owned());
        }
        if direct_receipt
            .and_then(|receipt| receipt.hook.as_ref())
            .is_some_and(|hook| hook.state == HookBindingState::Suspended)
        {
            return Self::suspended(snapshot, item, entry, target_id, action, index);
        }
        let target = if is_apply {
            hook_targets::resolve(snapshot, target_id, direct_receipt)?
        } else {
            let binding = item
                .bindings
                .iter()
                .find(|binding| binding.id == target_id)
                .ok_or("选中 Hook 绑定不属于该资产")?;
            hook_targets::for_binding(
                snapshot,
                &binding.native,
                matches!(
                    action,
                    AgentCatalogAction::Enable | AgentCatalogAction::Disable
                ),
            )?
        };
        if let Some(reason) =
            hook_targets::structural_reason(target.context, target.source, &target.destination)
        {
            return Err(reason);
        }
        if entry.receipts.values().any(|receipt| {
            receipt.context_id == target.context.id
                && receipt.hook.as_ref().is_some_and(|hook| {
                    hook.pending.is_some()
                        && (hook.destination.source_id == target.destination.source_id
                            || target.binding.is_some_and(|asset| {
                                hook.rule.source_id == asset.inspection_source_id
                            }))
                })
        }) {
            return Err("此 Hook 来源存在尚未完成对账的提交，不能继续写入同一恢复关系".to_owned());
        }
        let candidates = item
            .bindings
            .iter()
            .filter(|binding| {
                binding.native.context_id == target.context.id
                    && binding.native.inspection_source_id == target.destination.source_id
                    && binding.native.scope == target.destination.scope
            })
            .collect::<Vec<_>>();
        let original_asset = if let Some(binding) = target.binding {
            Some(binding)
        } else if let Some(receipt) = direct_receipt {
            receipt
                .hook
                .as_ref()
                .and_then(|hook| hook.rule.native_asset_id.as_deref())
                .and_then(|id| item.bindings.iter().find(|binding| binding.id == id))
                .map(|binding| &binding.native)
        } else {
            match candidates.as_slice() {
                [binding] => Some(&binding.native),
                [] => None,
                _ => return Err("此配置中有多个 Hook 绑定，请选择具体规则后应用".to_owned()),
            }
        };
        if let Some(asset) = original_asset {
            if !item
                .bindings
                .iter()
                .any(|binding| binding.id == asset.stable_id)
            {
                return Err("选中的原生 Hook 属于另一全局资产".to_owned());
            }
        }
        let original = original_asset
            .map(|asset| (target.adapter.read_hook)(snapshot, asset))
            .transpose()?;
        let existing = direct_receipt
            .map(|receipt| (target_id.to_owned(), receipt))
            .or_else(|| {
                original_asset.and_then(|asset| {
                    entry
                        .receipts
                        .iter()
                        .find(|(_, receipt)| {
                            super::super::repository::receipt_matches(
                                receipt,
                                asset,
                                super::super::projection::definition_source(snapshot, asset)
                                    .map(|source| source.path.as_str()),
                            )
                        })
                        .map(|(id, receipt)| (id.clone(), receipt))
                })
            });
        if existing
            .as_ref()
            .and_then(|(_, receipt)| receipt.hook.as_ref())
            .is_some_and(|hook| hook.pending.is_some())
        {
            return Err("Hook 应用记录尚未完成对账".to_owned());
        }
        let (desired, version) = if is_apply {
            let definition = entry.current().ok_or("请先保存共享 Hook 定义")?;
            (
                variant(definition, target.context.agent_kind)?.clone(),
                definition.version,
            )
        } else {
            (
                original
                    .as_ref()
                    .ok_or("原生 Hook 当前不存在")?
                    .definition
                    .clone(),
                existing.as_ref().map_or(0, |(_, receipt)| receipt.version),
            )
        };
        let edit = if is_apply {
            (target.adapter.validate_definition)(&desired)?;
            match &original {
                Some(original) => HookNativeEdit::Replace {
                    original: original.clone(),
                    definition: desired.clone(),
                },
                None => HookNativeEdit::Add {
                    definition: desired.clone(),
                },
            }
        } else {
            action_edit(
                target.adapter,
                original.as_ref().ok_or("原生 Hook 已缺失")?,
                action,
            )?
        };
        target.adapter.qualify(target.context, edit.capability())?;
        let prepared = (target.adapter.prepare_hook_edits)(
            snapshot,
            &target.destination,
            std::slice::from_ref(&edit),
        )?;
        let write_source_ids = prepared
            .writes
            .iter()
            .map(|write| write.source_id.clone())
            .collect();
        let policy_expectations = prepared.expectations;
        for capability in prepared.required_capabilities {
            target.adapter.qualify(target.context, capability)?;
        }
        let state = if action == AgentCatalogAction::Disable
            && target.adapter.switch_mode == HookNativeSwitchMode::Suspend
        {
            HookBindingState::Suspended
        } else {
            HookBindingState::Active
        };
        let rule = original
            .clone()
            .unwrap_or_else(|| unlocated_rule(&target.destination, &desired));
        let source = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| source.id == rule.source_id)
            .ok_or("Hook 原生来源不存在")?;
        let carrier = Receipt {
            context_id: target.context.id.clone(),
            scope: source.scope,
            agent_kind: target.context.agent_kind,
            path: source.path.clone(),
            name: original_asset
                .map(|asset| asset.native_id.clone())
                .unwrap_or_else(|| item.name.clone()),
            version,
            fingerprint: hook_definition::fingerprint(&desired),
            hook: Some(HookReceipt {
                destination: target.destination.clone(),
                rule,
                desired: desired.clone(),
                state,
                pending: None,
            }),
        };
        let key = existing
            .as_ref()
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| {
                stable_id(
                    "catalog-hook-receipt",
                    &[&item.id, target_id, &carrier.fingerprint],
                )
            });
        // Native toggles may also update a separate policy or ownership file.
        // Retain their complete intent even for observed, unadopted rules;
        // version zero records recovery without creating a shared definition.
        let transition = Some(ReceiptTransition {
            key,
            before: existing.as_ref().map(|(_, receipt)| (*receipt).clone()),
            after: (action != AgentCatalogAction::RemoveBinding).then(|| carrier.clone()),
            carrier,
        });
        let changes = changes(
            action,
            source,
            original.as_ref().map(|rule| &rule.definition),
            Some(&desired),
            state,
            version,
        );
        let signature = member_signature(
            target_id,
            action,
            target.context,
            &target.destination,
            original.as_ref(),
            &desired,
            transition.as_ref(),
        )?;
        Ok(Self {
            index,
            target_id: target_id.to_owned(),
            label: format!(
                "{} · {} · {}",
                definition(target.context.agent_kind).label,
                item.name,
                source.path
            ),
            context: target.context.clone(),
            destination: target.destination,
            adapter: target.adapter,
            edit: Some(edit),
            transition,
            write_source_ids,
            policy_expectations,
            changes,
            signature,
        })
    }

    fn suspended(
        snapshot: &MutationInventory,
        item: &AgentCatalogAsset,
        entry: &Entry,
        target_id: &str,
        action: AgentCatalogAction,
        index: usize,
    ) -> Result<Self, String> {
        let receipt = entry.receipts.get(target_id).ok_or("Hook 恢复记录缺失")?;
        let hook = receipt.hook.as_ref().ok_or("Hook 恢复记录缺失")?;
        let context = snapshot
            .inventory
            .contexts
            .iter()
            .find(|context| {
                context.id == receipt.context_id && context.agent_kind == receipt.agent_kind
            })
            .ok_or("Hook 恢复上下文当前不可见")?;
        let adapter = definition(receipt.agent_kind)
            .environment
            .hook_adapter()
            .ok_or("此 Agent 尚未注册 Hook 管理")?;
        let mut after = receipt.clone();
        let mut desired = hook.desired.clone();
        let mut version = receipt.version;
        let edit = match action {
            AgentCatalogAction::ApplyDefinition | AgentCatalogAction::Enable => {
                let target = hook_targets::resolve(snapshot, target_id, Some(receipt))?;
                if let Some(reason) =
                    hook_targets::structural_reason(context, target.source, &target.destination)
                {
                    return Err(reason);
                }
                adapter.qualify(context, HookNativeCapability::Configuration)?;
                if action == AgentCatalogAction::ApplyDefinition {
                    let definition = entry.current().ok_or("请先保存共享 Hook 定义")?;
                    desired = variant(definition, receipt.agent_kind)?.clone();
                    version = definition.version;
                    (adapter.validate_definition)(&desired)?;
                    None
                } else {
                    (adapter.validate_definition)(&desired)?;
                    Some(HookNativeEdit::Restore {
                        original: hook.rule.clone(),
                        definition: desired.clone(),
                    })
                }
            }
            AgentCatalogAction::RemoveBinding => None,
            AgentCatalogAction::Disable => return Err("此 Hook 已暂停，恢复副本已保留".to_owned()),
        };
        let (write_source_ids, policy_expectations) = if let Some(edit) = &edit {
            let prepared = (adapter.prepare_hook_edits)(
                snapshot,
                &hook.destination,
                std::slice::from_ref(edit),
            )?;
            for capability in prepared.required_capabilities {
                adapter.qualify(context, capability)?;
            }
            (
                prepared
                    .writes
                    .iter()
                    .map(|write| write.source_id.clone())
                    .collect(),
                prepared.expectations,
            )
        } else {
            (BTreeSet::new(), Vec::new())
        };
        let state = if action == AgentCatalogAction::Enable {
            HookBindingState::Active
        } else {
            HookBindingState::Suspended
        };
        after.version = version;
        after.fingerprint = hook_definition::fingerprint(&desired);
        let next = after.hook.as_mut().ok_or("Hook 恢复记录无效")?;
        next.desired = desired.clone();
        next.state = state;
        let source = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| source.id == hook.rule.source_id)
            .ok_or("Hook 原生配置来源当前不可见")?;
        let changes = changes(
            action,
            source,
            Some(&hook.desired),
            Some(&desired),
            state,
            version,
        );
        let transition = ReceiptTransition {
            key: target_id.to_owned(),
            before: Some(receipt.clone()),
            after: (action != AgentCatalogAction::RemoveBinding).then_some(after),
            carrier: receipt.clone(),
        };
        let signature = member_signature(
            target_id,
            action,
            context,
            &hook.destination,
            Some(&hook.rule),
            &desired,
            Some(&transition),
        )?;
        Ok(Self {
            index,
            target_id: target_id.to_owned(),
            label: format!(
                "{} · {} · 已暂停",
                definition(receipt.agent_kind).label,
                item.name
            ),
            context: context.clone(),
            destination: hook.destination.clone(),
            adapter,
            edit,
            transition: Some(transition),
            write_source_ids,
            policy_expectations,
            changes,
            signature,
        })
    }
}

pub(super) fn action_edit(
    adapter: &NativeHookAdapter,
    original: &HookNativeRule,
    action: AgentCatalogAction,
) -> Result<HookNativeEdit, String> {
    match action {
        AgentCatalogAction::Enable if adapter.switch_mode == HookNativeSwitchMode::Native => {
            Ok(HookNativeEdit::SetEnabled {
                original: original.clone(),
                enabled: true,
            })
        }
        AgentCatalogAction::Disable if adapter.switch_mode == HookNativeSwitchMode::Native => {
            Ok(HookNativeEdit::SetEnabled {
                original: original.clone(),
                enabled: false,
            })
        }
        AgentCatalogAction::Disable | AgentCatalogAction::RemoveBinding => {
            Ok(HookNativeEdit::Remove {
                original: original.clone(),
            })
        }
        AgentCatalogAction::Enable => {
            Err("该原生 Hook 已在配置中；仅已暂停的恢复副本可以恢复".to_owned())
        }
        AgentCatalogAction::ApplyDefinition => Err("应用 Hook 定义必须选择共享版本".to_owned()),
    }
}

fn variant(
    definition: &StoredDefinition,
    kind: AgentCliKind,
) -> Result<&HookNativeDefinition, String> {
    match &definition.payload {
        DefinitionPayload::Hook(variants) => variants
            .get(&kind)
            .ok_or_else(|| "请先添加此 Agent 的原生 Hook 定义".to_owned()),
        _ => Err("共享定义不是 Hook".to_owned()),
    }
}

fn unlocated_rule(
    destination: &HookNativeDestination,
    definition: &HookNativeDefinition,
) -> HookNativeRule {
    HookNativeRule {
        source_id: destination.source_id.clone(),
        native_asset_id: None,
        definition: definition.clone(),
        anchor: HookNativeAnchor {
            event: definition.event.clone(),
            group_index: 0,
            handler_index: 0,
            original_group: definition.group.clone(),
        },
        enabled: AgentAssetDeclaredState::Enabled,
    }
}

fn member_signature(
    target_id: &str,
    action: AgentCatalogAction,
    context: &AgentConfigurationContext,
    destination: &HookNativeDestination,
    original: Option<&HookNativeRule>,
    desired: &HookNativeDefinition,
    transition: Option<&ReceiptTransition>,
) -> Result<String, String> {
    serde_json::to_vec(&(
        target_id,
        action,
        context,
        destination,
        original,
        desired,
        transition.map(|value| (&value.key, &value.before, &value.after)),
    ))
    .map(|bytes| digest(&bytes))
    .map_err(|_| "Hook 计划身份无效".to_owned())
}

fn changes(
    action: AgentCatalogAction,
    source: &AgentAssetSource,
    before: Option<&HookNativeDefinition>,
    after: Option<&HookNativeDefinition>,
    state: HookBindingState,
    version: u64,
) -> Vec<AgentAssetPlanChange> {
    // Preview uses the same original field values as the definition editor.
    let render = |definition: &HookNativeDefinition| {
        let value = BTreeMap::from([(AgentCliKind::ClaudeCode, definition.clone())]);
        hook_definition::public(&value)
            .variants
            .first()
            .map(|variant| format!("{}\n{}", variant.event, variant.group_json))
    };
    vec![AgentAssetPlanChange {
        label: match action {
            AgentCatalogAction::ApplyDefinition if state == HookBindingState::Suspended => {
                format!("更新暂停副本至版本 {version}；保持停用")
            }
            AgentCatalogAction::ApplyDefinition => format!("应用原生 Hook 版本 {version}"),
            AgentCatalogAction::Enable => "启用所选 Hook；保留原生信任策略".to_owned(),
            AgentCatalogAction::Disable if state == HookBindingState::Suspended => {
                "移除所选原生规则并保留完整恢复副本".to_owned()
            }
            AgentCatalogAction::Disable => "停用所选 Hook 的原生策略".to_owned(),
            AgentCatalogAction::RemoveBinding => {
                "从此 Agent 移除所选 Hook；保留共享定义和其他绑定".to_owned()
            }
        },
        path: Some(source.path.clone()),
        before: before.and_then(render),
        after: if action == AgentCatalogAction::RemoveBinding {
            None
        } else {
            after.and_then(render)
        },
    }]
}

impl PreparedHookGroup {
    pub fn prepare(
        service: &CatalogService,
        snapshot: &MutationInventory,
        inspector: &dyn MutationInspector,
        members: Vec<HookMember>,
    ) -> Result<Self, String> {
        let mut batches = BTreeMap::<String, (usize, Vec<usize>, Vec<HookNativeEdit>)>::new();
        let mut reads = BTreeMap::new();
        let mut writes = BTreeMap::<String, HookWrite>::new();
        let mut expectations = Vec::new();
        let mut policy_changes = Vec::new();
        let mut affected = BTreeSet::new();
        let mut notes = BTreeSet::new();
        let mut domains = BTreeSet::new();
        for (index, member) in members.iter().enumerate() {
            domains.insert(format!("config:{}", member.context.config_root));
            if let Some(edit) = &member.edit {
                let key = format!(
                    "{}:{}:{:?}",
                    member.destination.source_id, member.destination.role, member.destination.scope
                );
                let batch = batches
                    .entry(key)
                    .or_insert_with(|| (index, Vec::new(), Vec::new()));
                batch.1.push(member.index);
                batch.2.push(edit.clone());
            }
        }
        for (index, indexes, edits) in batches.into_values() {
            let member = &members[index];
            let prepared =
                (member.adapter.prepare_hook_edits)(snapshot, &member.destination, &edits)?;
            for &capability in &prepared.required_capabilities {
                member.adapter.qualify(&member.context, capability)?;
            }
            if prepared.expectations.is_empty() {
                return Err("原生 Hook 计划没有可验证的结果证据".to_owned());
            }
            if !prepared.writes.is_empty() {
                // Adapter-owned exact expectations identify policy peers. A
                // Replace may implicitly preserve disabled state under a new
                // identity. Only actual state changes transfer policy ownership;
                // index rebasing alone never establishes an ownership impact.
                for asset in snapshot.inventory.assets.iter().filter(|asset| {
                    asset.category == AgentAssetCategory::Hook
                        && prepared.affected_asset_ids.contains(&asset.stable_id)
                }) {
                    let rule = (member.adapter.read_hook)(snapshot, asset)?;
                    let changed_state = prepared.expectations.iter().find_map(|expectation| {
                        (expectation.source_id == rule.source_id
                            && expectation.definition == rule.definition
                            && expectation.occurrences > 0)
                            .then_some(expectation.enabled)
                            .flatten()
                            .filter(|state| *state != rule.enabled)
                    });
                    if let Some(state) = changed_state {
                        let owners = members
                            .iter()
                            .filter(|candidate| {
                                indexes.contains(&candidate.index)
                                    && candidate.policy_expectations.iter().any(|expectation| {
                                        expectation.source_id == rule.source_id
                                            && expectation.definition == rule.definition
                                            && expectation.occurrences > 0
                                            && expectation.enabled == Some(state)
                                    })
                            })
                            .map(|candidate| candidate.index)
                            .collect::<Vec<_>>();
                        if owners.is_empty() {
                            return Err(
                                "复合 Hook 策略影响无法归属到所选目标，请分别计划".to_owned()
                            );
                        }
                        policy_changes.push((owners, member.context.agent_kind, rule));
                    }
                }
            }
            let source_ids = prepared
                .read_source_ids
                .iter()
                .chain(prepared.writes.iter().map(|write| &write.source_id))
                .collect::<BTreeSet<_>>();
            for id in source_ids {
                let source = snapshot
                    .inventory
                    .sources
                    .iter()
                    .find(|source| &source.id == id)
                    .ok_or("Hook 依赖来源不存在")?;
                reads
                    .entry(id.clone())
                    .or_insert(HookReadGuard::capture(snapshot, source)?);
            }
            for write in prepared.writes {
                let write_indexes = members
                    .iter()
                    .filter(|candidate| {
                        indexes.contains(&candidate.index)
                            && candidate.write_source_ids.contains(&write.source_id)
                    })
                    .map(|candidate| candidate.index)
                    .collect::<Vec<_>>();
                if write_indexes.is_empty() {
                    return Err("复合 Hook 写入无法归属到所选目标，请分别计划".to_owned());
                }
                let source = snapshot
                    .inventory
                    .sources
                    .iter()
                    .find(|source| source.id == write.source_id)
                    .ok_or("Hook 写入来源不存在")?;
                if !source.writable
                    || source.revision.is_symlink
                    || !matches!(
                        source.scope,
                        AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
                    )
                {
                    return Err("Hook 原生计划包含只读或未授权的写入来源".to_owned());
                }
                let root = writable_root(inspector, source)?;
                let file = GuardedFile::capture(source, &snapshot.source_anchors)
                    .map_err(|_| "Hook 写入目标无法安全准备")?;
                let key = file.domain();
                if let Some(existing) = writes.get_mut(&key) {
                    if existing.file.path() != file.path()
                        || existing.file.bytes() != file.bytes()
                        || existing.bytes != write.bytes
                    {
                        return Err("多个目标共享同一物理 Hook 文件，但原生复合结果不一致；请分别确认其实际配置来源".to_owned());
                    }
                    existing.indexes.extend(&write_indexes);
                    existing.indexes.sort_unstable();
                    existing.indexes.dedup();
                } else {
                    writes.insert(
                        key,
                        HookWrite {
                            indexes: write_indexes,
                            file,
                            bytes: write.bytes,
                            root,
                            evidence_root: source.allowed_root.clone(),
                            source_id: Some(write.source_id),
                        },
                    );
                }
            }
            expectations.extend(
                prepared
                    .expectations
                    .into_iter()
                    .map(|expectation| (member.context.agent_kind, expectation)),
            );
            affected.extend(prepared.affected_asset_ids);
            notes.extend(prepared.notes);
        }
        if let Some(root) = &service.managed_hook_root {
            let mut auxiliary = Vec::new();
            for write in writes.values() {
                for kind in members
                    .iter()
                    .filter(|member| {
                        write.source_id.as_ref().is_some_and(|source_id| {
                            snapshot.inventory.sources.iter().any(|source| {
                                &source.id == source_id && source.context_id == member.context.id
                            })
                        })
                    })
                    .map(|member| member.context.agent_kind)
                    .collect::<BTreeSet<_>>()
                {
                    if let Some(change) = managed_hook::prepare_catalog_change(
                        root,
                        kind,
                        write.file.path(),
                        write.file.bytes().unwrap_or_default(),
                        &write.bytes,
                    )? {
                        notes.insert(change.note);
                        auxiliary.push(HookWrite {
                            indexes: write.indexes.clone(),
                            file: change.file,
                            bytes: change.replacement,
                            root: root.clone(),
                            evidence_root: root.to_string_lossy().into_owned(),
                            source_id: None,
                        });
                    }
                }
            }
            for (indexes, kind, rule) in policy_changes {
                let source = snapshot
                    .inventory
                    .sources
                    .iter()
                    .find(|source| source.id == rule.source_id)
                    .ok_or("Hook 开关影响来源缺失")?;
                reads
                    .entry(source.id.clone())
                    .or_insert(HookReadGuard::capture(snapshot, source)?);
                let before = super::super::native::hook_codec::observe_bytes(snapshot, &source.id)?
                    .ok_or("Hook 开关影响规则已缺失")?;
                if let Some(change) = managed_hook::prepare_catalog_policy_change(
                    root,
                    kind,
                    Path::new(&source.path),
                    &before,
                    &rule.anchor.event,
                    &rule.anchor.original_group,
                )? {
                    notes.insert(change.note);
                    auxiliary.push(HookWrite {
                        indexes,
                        file: change.file,
                        bytes: change.replacement,
                        root: root.clone(),
                        evidence_root: root.to_string_lossy().into_owned(),
                        source_id: None,
                    });
                }
            }
            for write in auxiliary {
                let key = write.file.domain();
                if let Some(existing) = writes.get_mut(&key) {
                    if existing.bytes != write.bytes {
                        return Err("会话接入所有权记录存在冲突".to_owned());
                    }
                    existing.indexes.extend(write.indexes);
                    existing.indexes.sort_unstable();
                    existing.indexes.dedup();
                } else {
                    writes.insert(key, write);
                }
            }
        }
        for read in reads.values() {
            domains.extend(read.lock_domains());
        }
        domains.extend(writes.values().flat_map(|write| write.file.lock_domains()));
        // Disclose current observations in every context sharing a physical
        // source. A filename alias cannot conceal impact on another Agent row.
        for source in &snapshot.inventory.sources {
            if source.source_kind != AgentAssetSourceKind::File {
                continue;
            }
            if let Ok(file) = GuardedFile::capture(source, &snapshot.source_anchors) {
                if writes.contains_key(&file.domain()) {
                    affected.extend(
                        snapshot
                            .inventory
                            .assets
                            .iter()
                            .filter(|asset| asset.source_ids.contains(&source.id))
                            .map(|asset| asset.stable_id.clone()),
                    );
                }
            }
        }
        let writes = writes.into_values().collect::<Vec<_>>();
        let signature = digest(
            &serde_json::to_vec(&(
                members
                    .iter()
                    .map(|member| &member.signature)
                    .collect::<Vec<_>>(),
                reads
                    .iter()
                    .map(|(id, guard)| (id, guard.signature()))
                    .collect::<Vec<_>>(),
                writes
                    .iter()
                    .map(|write| (&write.indexes, write.file.signature(), digest(&write.bytes)))
                    .collect::<Vec<_>>(),
            ))
            .map_err(|_| "Hook 复合计划无效")?,
        );
        Ok(Self {
            members,
            reads,
            writes,
            expectations,
            affected_asset_ids: affected.into_iter().collect(),
            notes: notes.into_iter().collect(),
            domains: domains.into_iter().collect(),
            signature,
        })
    }

    pub fn revalidate(&self) -> Result<(), String> {
        for guard in self.reads.values() {
            guard.revalidate()?;
        }
        for write in &self.writes {
            write.file.revalidate().map_err(|_| "Hook 写入目标已变化")?;
        }
        Ok(())
    }

    pub fn evidence(&self, index: usize) -> Vec<HookFileEvidence> {
        self.writes
            .iter()
            .filter(|write| write.indexes.contains(&index))
            .map(|write| HookFileEvidence {
                source_id: write.source_id.clone(),
                root: write.evidence_root.clone(),
                path: write.file.path().to_string_lossy().into_owned(),
                before: write.file.bytes().map(digest),
                after: digest(&write.bytes),
            })
            .collect()
    }
}

fn writable_root(
    inspector: &dyn MutationInspector,
    source: &AgentAssetSource,
) -> Result<PathBuf, String> {
    let allowed = Path::new(&source.allowed_root);
    if allowed.starts_with(inspector.home()) {
        Ok(inspector.home().to_path_buf())
    } else if inspector
        .workspace()
        .is_some_and(|workspace| allowed.starts_with(workspace))
    {
        inspector
            .workspace()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Hook 工作区已失效".to_owned())
    } else {
        Err("Hook 写入不在当前用户或所选项目的原生可写范围".to_owned())
    }
}

pub(super) fn binding_actions(
    reader: &mut super::super::observation::DefinitionReader<'_>,
    asset: &AgentAssetRecord,
) -> Vec<AgentAssetAction> {
    let snapshot = reader.snapshot;
    let original = reader.hook(asset);
    let mut actions = asset
        .actions
        .iter()
        .filter(|action| {
            !matches!(
                action.action,
                AgentAssetActionKind::Enable
                    | AgentAssetActionKind::Disable
                    | AgentAssetActionKind::Remove
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for (action, catalog_action) in [
        (AgentAssetActionKind::Enable, AgentCatalogAction::Enable),
        (AgentAssetActionKind::Disable, AgentCatalogAction::Disable),
        (
            AgentAssetActionKind::Remove,
            AgentCatalogAction::RemoveBinding,
        ),
    ] {
        let result = (|| {
            let target =
                hook_targets::for_binding(snapshot, asset, action != AgentAssetActionKind::Remove)?;
            if let Some(reason) =
                hook_targets::structural_reason(target.context, target.source, &target.destination)
            {
                return Err(reason);
            }
            let original = original.as_ref().map_err(Clone::clone)?;
            let edit = action_edit(target.adapter, original, catalog_action)?;
            let capability = edit.capability();
            // A list admits entry into planning; it must not prepare all three
            // possible file mutations for every row. The plan still prepares
            // the exact edit and qualifies ALL policy-dependent capabilities.
            target.adapter.qualify(target.context, capability)
        })();
        actions.push(public_action(asset.agent_kind, action, result));
    }
    actions
}

pub(super) fn suspended_actions(
    snapshot: &MutationInventory,
    receipt: &Receipt,
) -> Vec<AgentAssetAction> {
    let ready = receipt
        .hook
        .as_ref()
        .filter(|hook| hook.state == HookBindingState::Suspended && hook.pending.is_none());
    [AgentAssetActionKind::Enable, AgentAssetActionKind::Remove]
        .into_iter()
        .map(|action| {
            let result = (|| {
                let hook = ready.ok_or("Hook 恢复副本尚未完成事实对账")?;
                let target = hook_targets::resolve(snapshot, "", Some(receipt))?;
                if action == AgentAssetActionKind::Remove {
                    return Ok(());
                }
                if let Some(reason) = hook_targets::structural_reason(
                    target.context,
                    target.source,
                    &target.destination,
                ) {
                    return Err(reason);
                }
                (target.adapter.validate_definition)(&hook.desired)?;
                target
                    .adapter
                    .qualify(target.context, HookNativeCapability::Configuration)
            })();
            public_action(receipt.agent_kind, action, result)
        })
        .collect()
}

fn public_action(
    kind: AgentCliKind,
    action: AgentAssetActionKind,
    result: Result<(), String>,
) -> AgentAssetAction {
    let reason = result.as_ref().err().map(|message| {
        if message.contains("信任") {
            AgentAssetActionUnavailableReason::TrustRequired
        } else {
            AgentAssetActionUnavailableReason::SourceUnavailable
        }
    });
    AgentAssetAction {
        action,
        available: result.is_ok(),
        reason,
        mechanism_id: Some(format!("hook-catalog:{}:{action:?}", kind.key())),
        confirmation_required: true,
        reload_effect: Some(
            result
                .err()
                .unwrap_or_else(|| "新会话或 Agent 原生重载后生效；不会执行 Hook 内容".to_owned()),
        ),
        trust_effect: Some("保留原生信任、系统策略与插件归属".to_owned()),
        selected_installation_id: None,
        risks: Vec::new(),
    }
}
