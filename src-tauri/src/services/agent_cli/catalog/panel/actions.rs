//! Contextual actions shared by Agent popovers and asset management details.
use crate::{models::*, services::agent_cli::environment::mutation::MutationInventory};

pub(super) fn panel_action(
    action: AgentCatalogAction,
    target: &AgentCatalogTargetPlan,
    parent_native_asset_id: Option<String>,
    label: String,
) -> AgentCatalogAgentPanelAction {
    AgentCatalogAgentPanelAction {
        action,
        label,
        target_ids: vec![target.target_id.clone()],
        available: target.available,
        reason: target.reason.clone(),
        affected_asset_ids: target.affected_asset_ids.clone(),
        parent_native_asset_id,
    }
}

pub(super) fn visible_action(
    snapshot: &MutationInventory,
    item: &AgentCatalogAsset,
    id: &str,
    action: AgentCatalogAction,
) -> bool {
    super::super::action_state::toggle_blocker(snapshot, item, id, action).is_none()
}

pub(super) fn action_label(
    snapshot: &MutationInventory,
    item: &AgentCatalogAsset,
    id: &str,
    action: AgentCatalogAction,
) -> String {
    if action == AgentCatalogAction::RemoveBinding {
        return "移除此配置".to_owned();
    }
    let kind = if action == AgentCatalogAction::Enable {
        AgentAssetActionKind::Enable
    } else {
        AgentAssetActionKind::Disable
    };
    let parent = item.category != AgentAssetCategory::Hook
        && item
            .bindings
            .iter()
            .find(|binding| binding.id == id)
            .is_some_and(|binding| {
                super::super::action_state::action_owner(snapshot, binding, kind).stable_id
                    != binding.id
            });
    match (action, parent) {
        (AgentCatalogAction::Enable, true) => "启用所属插件",
        (AgentCatalogAction::Disable, true) => "停用所属插件",
        (AgentCatalogAction::Enable, false) => "启用",
        _ => "停用",
    }
    .to_owned()
}

pub(super) fn batch_actions(
    entries: &[AgentCatalogAgentPanelEntry],
) -> Vec<AgentCatalogAgentPanelAction> {
    let mut groups: Vec<AgentCatalogAgentPanelAction> = Vec::new();
    for capability in entries
        .iter()
        .filter(|entry| entry.target_kind != AgentCatalogTargetKind::Destination)
        .flat_map(|entry| &entry.actions)
        .filter(|action| action.available && action.action != AgentCatalogAction::ApplyDefinition)
    {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.action == capability.action && group.label == capability.label)
        {
            for id in &capability.target_ids {
                if !group.target_ids.contains(id) {
                    group.target_ids.push(id.clone());
                }
            }
            for id in &capability.affected_asset_ids {
                if !group.affected_asset_ids.contains(id) {
                    group.affected_asset_ids.push(id.clone());
                }
            }
        } else {
            let mut group = capability.clone();
            group.parent_native_asset_id = None;
            group.reason = None;
            groups.push(group);
        }
    }
    groups.retain(|group| group.target_ids.len() > 1);
    for group in &mut groups {
        let label = if group.action == AgentCatalogAction::RemoveBinding {
            "移除配置"
        } else {
            &group.label
        };
        group.label = format!("批量{}（{}）", label, group.target_ids.len());
    }
    groups
}
