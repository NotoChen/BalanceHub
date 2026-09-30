use super::{
    native::hooks::{HookNativeCapability, HookNativeDestination, NativeHookAdapter},
    repository::{Library, Receipt},
};
use crate::{
    models::*,
    services::agent_cli::{
        definition,
        environment::{mutation::MutationInventory, stable_id},
    },
};

pub(super) struct HookTarget<'a> {
    pub context: &'a AgentConfigurationContext,
    pub source: &'a AgentAssetSource,
    pub adapter: &'static NativeHookAdapter,
    pub destination: HookNativeDestination,
    pub binding: Option<&'a AgentAssetRecord>,
}

fn target_id(context: &AgentConfigurationContext, destination: &HookNativeDestination) -> String {
    stable_id(
        "catalog-hook-target",
        &[
            &context.id,
            &destination.source_id,
            &destination.role,
            &format!("{:?}", destination.scope),
        ],
    )
}

fn member_target_id(asset: &AgentAssetRecord) -> String {
    stable_id("catalog-hook-member", &[&asset.stable_id])
}

pub(super) fn structural_reason(
    context: &AgentConfigurationContext,
    source: &AgentAssetSource,
    destination: &HookNativeDestination,
) -> Option<String> {
    if !cfg!(unix) {
        return Some("此平台尚未验证 Hook 原子文件写入".to_owned());
    }
    if source.context_id != context.id
        || source.environment_id != context.environment_id
        || source.id != destination.source_id
        || !source.writable
        || source.revision.is_symlink
        || source.source_kind != AgentAssetSourceKind::File
        || !source.categories.contains(&AgentAssetCategory::Hook)
        || !matches!(
            destination.scope,
            AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
        )
    {
        return Some("Hook 目标不是当前上下文已确认的原生可写配置".to_owned());
    }
    if matches!(
        destination.scope,
        AgentAssetScope::Workspace | AgentAssetScope::Local
    ) && context.trust_context != AgentTrustState::Trusted
    {
        return Some("请先在 Agent 原生工具中信任该项目".to_owned());
    }
    None
}

fn public_target(target: &HookTarget<'_>, id: String, name: Option<&str>) -> AgentCatalogTarget {
    let reason =
        structural_reason(target.context, target.source, &target.destination).or_else(|| {
            target
                .adapter
                .qualify(target.context, HookNativeCapability::Configuration)
                .err()
        });
    AgentCatalogTarget {
        id,
        context_id: target.context.id.clone(),
        agent_kind: target.context.agent_kind,
        scope: target.destination.scope,
        label: format!(
            "{} · {} · Hook{} · {}",
            definition(target.context.agent_kind).label,
            match target.destination.scope {
                AgentAssetScope::User => "用户",
                AgentAssetScope::Local => "项目本地",
                _ => "项目",
            },
            name.map(|name| format!(" · {name}")).unwrap_or_default(),
            target.source.path
        ),
        categories: vec![AgentAssetCategory::Hook],
        available: reason.is_none(),
        reason,
    }
}

pub(super) fn targets(inventory: &AgentEnvironmentInventory) -> Vec<AgentCatalogTarget> {
    let mut output = Vec::new();
    for context in &inventory.contexts {
        let Some(adapter) = definition(context.agent_kind).environment.hook_adapter() else {
            continue;
        };
        for destination in (adapter.hook_targets)(inventory, context) {
            let Some(source) = inventory
                .sources
                .iter()
                .find(|source| source.id == destination.source_id)
            else {
                continue;
            };
            let id = target_id(context, &destination);
            let target = HookTarget {
                context,
                source,
                adapter,
                destination,
                binding: None,
            };
            output.push(public_target(&target, id, None));
            for asset in inventory.assets.iter().filter(|asset| {
                asset.category == AgentAssetCategory::Hook
                    && asset.context_id == context.id
                    && asset.inspection_source_id == source.id
                    && asset.scope == target.destination.scope
            }) {
                output.push(public_target(
                    &target,
                    member_target_id(asset),
                    Some(&asset.label),
                ));
            }
        }
    }
    output
}

pub(super) fn append_receipt_targets(
    targets: &mut Vec<AgentCatalogTarget>,
    library: &Library,
    snapshot: &MutationInventory,
) {
    for entry in library
        .entries
        .values()
        .filter(|entry| entry.category == AgentAssetCategory::Hook)
    {
        for (id, receipt) in &entry.receipts {
            if let Ok(target) = resolve(snapshot, id, Some(receipt)) {
                let mut row = public_target(&target, id.clone(), Some(&entry.name));
                if receipt
                    .hook
                    .as_ref()
                    .is_some_and(|hook| hook.pending.is_some())
                {
                    row.available = false;
                    row.reason =
                        Some("上次 Hook 提交尚未完成事实对账，请先核实原生配置".to_owned());
                }
                if let Some(existing) = targets.iter_mut().find(|target| target.id == row.id) {
                    // A receipt and a native destination can identify the same
                    // target. Keep one choice, and preserve any pending barrier
                    // regardless of which entry supplied the other receipt.
                    if !row.available && existing.available {
                        existing.available = false;
                        existing.reason = row.reason;
                    }
                } else {
                    targets.push(row);
                }
            }
        }
    }
}

pub(super) fn resolve<'a>(
    snapshot: &'a MutationInventory,
    id: &str,
    receipt: Option<&Receipt>,
) -> Result<HookTarget<'a>, String> {
    for context in &snapshot.inventory.contexts {
        let Some(adapter) = definition(context.agent_kind).environment.hook_adapter() else {
            continue;
        };
        for destination in (adapter.hook_targets)(&snapshot.inventory, context) {
            let Some(source) = snapshot
                .inventory
                .sources
                .iter()
                .find(|source| source.id == destination.source_id)
            else {
                continue;
            };
            if let Some(receipt) = receipt {
                let Some(hook) = &receipt.hook else {
                    continue;
                };
                if receipt.context_id == context.id
                    && receipt.agent_kind == context.agent_kind
                    && receipt.path == source.path
                    && receipt.scope == destination.scope
                    && hook.destination == destination
                {
                    return Ok(HookTarget {
                        context,
                        source,
                        adapter,
                        destination,
                        binding: None,
                    });
                }
            } else {
                let binding = snapshot.inventory.assets.iter().find(|asset| {
                    asset.category == AgentAssetCategory::Hook
                        && asset.context_id == context.id
                        && asset.inspection_source_id == source.id
                        && asset.scope == destination.scope
                        && member_target_id(asset) == id
                });
                if binding.is_some() || target_id(context, &destination) == id {
                    return Ok(HookTarget {
                        context,
                        source,
                        adapter,
                        destination,
                        binding,
                    });
                }
            }
        }
    }
    Err("Hook 应用目标不存在或已离开当前原生可写上下文".to_owned())
}

pub(super) fn for_binding<'a>(
    snapshot: &'a MutationInventory,
    asset: &'a AgentAssetRecord,
    allow_policy: bool,
) -> Result<HookTarget<'a>, String> {
    let context = snapshot
        .inventory
        .contexts
        .iter()
        .find(|context| context.id == asset.context_id)
        .ok_or("Hook 配置上下文不存在")?;
    let adapter = definition(asset.agent_kind)
        .environment
        .hook_adapter()
        .ok_or("该 Agent 未注册原生 Hook 管理")?;
    let destinations = (adapter.hook_targets)(&snapshot.inventory, context);
    let destination = destinations
        .iter()
        .find(|target| {
            target.source_id == asset.inspection_source_id && target.scope == asset.scope
        })
        .or_else(|| {
            allow_policy
                .then(|| {
                    destinations
                        .iter()
                        .find(|target| target.scope == AgentAssetScope::User)
                })
                .flatten()
        })
        .cloned()
        .ok_or("此 Hook 定义来源只读且没有可用的独立原生控制位置")?;
    let source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == destination.source_id)
        .ok_or("Hook 原生目标已变化")?;
    Ok(HookTarget {
        context,
        source,
        adapter,
        destination,
        binding: Some(asset),
    })
}
