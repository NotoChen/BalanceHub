//! Display identity for verified aliases. Native records and action targets
//! remain independent so removing a link never implies removing its target.
use super::identity::CurrentDefinition;
use crate::{models::*, services::agent_cli::environment::stable_id};
use std::collections::BTreeMap;

pub(super) fn single_group<'a>(
    catalog: &'a AgentAssetCatalog,
    records: &[&AgentAssetRecord],
) -> Option<&'a AgentCatalogBindingUsage> {
    let usage_for = |id: &str| {
        catalog
            .assets
            .iter()
            .flat_map(|item| &item.bindings)
            .find(|binding| binding.id == id)
            .and_then(|binding| binding.usage.as_ref())
    };
    let first = usage_for(&records.first()?.stable_id)?;
    records
        .iter()
        .all(|record| {
            usage_for(&record.stable_id).is_some_and(|usage| usage.group_id == first.group_id)
        })
        .then_some(first)
}

pub(super) fn group_bindings(
    item: &mut AgentCatalogAsset,
    observations: &BTreeMap<String, CurrentDefinition>,
) {
    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (index, binding) in item.bindings.iter().enumerate() {
        let native = &binding.native;
        let Some(physical) = observations
            .get(&binding.id)
            .and_then(|observation| observation.physical_key.as_deref())
        else {
            continue;
        };
        // The reader's physical identity includes the exact native declaration
        // for document resources. A file containing several MCP servers, Hook
        // handlers or plugin registrations must never become a single usage.
        // Scope and parent ownership also remain separate control boundaries.
        let key = stable_id(
            "asset-usage",
            &[
                &native.category.key(),
                native.agent_kind.key(),
                &native.context_id,
                &format!("{:?}", native.scope),
                &native.native_id,
                native
                    .relationships
                    .provided_by
                    .as_deref()
                    .unwrap_or_default(),
                native
                    .relationships
                    .action_owner
                    .as_deref()
                    .unwrap_or_default(),
                physical,
            ],
        );
        groups.entry(key).or_default().push(index);
    }
    for (group_id, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let active = indices
            .iter()
            .copied()
            .filter(|index| {
                item.bindings[*index].native.effective_state != AgentAssetState::Shadowed
            })
            .collect::<Vec<_>>();
        let primary = active.first().copied().unwrap_or(indices[0]);
        let state = item.bindings[primary].native.effective_state;
        let state = if active
            .iter()
            .all(|index| item.bindings[*index].native.effective_state == state)
        {
            state
        } else {
            AgentAssetState::Unknown
        };
        let linked = indices.iter().any(|index| {
            item.bindings[*index]
                .native
                .provenance
                .iter()
                .any(|origin| origin.installation == AgentAssetInstallationOrigin::Linked)
        });
        let shared = indices.iter().any(|index| {
            item.bindings[*index]
                .native
                .provenance
                .iter()
                .any(|origin| origin.installation == AgentAssetInstallationOrigin::SharedFiles)
        });
        let resource = match item.category {
            AgentAssetCategory::Skill => "Skill",
            AgentAssetCategory::Mcp => "MCP 配置",
            AgentAssetCategory::Hook => "Hook 规则",
            AgentAssetCategory::Plugin => "插件定义",
            AgentAssetCategory::Extension => "扩展定义",
            AgentAssetCategory::StatusUi => "状态栏配置",
        };
        let detail = if item.category == AgentAssetCategory::Skill && linked && shared {
            "共享 Skill，通过 Agent 目录链接引用".to_owned()
        } else {
            format!("同一份{resource}，通过多个入口发现")
        };
        let usage = AgentCatalogBindingUsage {
            group_id,
            primary_binding_id: item.bindings[primary].id.clone(),
            state,
            detail: format!("{detail}；{} 个入口指向同一物理来源", indices.len()),
        };
        for index in indices {
            item.bindings[index].usage = Some(usage.clone());
        }
    }
}
