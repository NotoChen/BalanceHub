//! State prerequisites shared by action display, target selection and preparation.
use crate::{
    models::*,
    services::agent_cli::environment::{mutation::MutationInventory, toggle_action_is_relevant},
};

pub(super) fn toggle_blocker(
    snapshot: &MutationInventory,
    item: &AgentCatalogAsset,
    id: &str,
    action: AgentCatalogAction,
) -> Option<String> {
    let kind = match action {
        AgentCatalogAction::Enable => AgentAssetActionKind::Enable,
        AgentCatalogAction::Disable => AgentAssetActionKind::Disable,
        _ => return None,
    };
    let state = if let Some(binding) = item.bindings.iter().find(|binding| binding.id == id) {
        if item.category != AgentAssetCategory::Hook
            && action_owner(snapshot, binding, kind).effective_state
                != binding.native.effective_state
        {
            return Some("此配置由所属插件控制，请查看所属插件的状态与影响范围".to_owned());
        }
        // Hook policy files can be edited without a runnable installation.
        // Preserve uncertainty about runtime adoption while using the parsed
        // native switch for configuration operations; codec/policy gates still
        // validate the actual write during preparation.
        if item.category == AgentAssetCategory::Hook
            && binding.native.effective_state == AgentAssetState::Unknown
        {
            binding.native.declared_state
        } else {
            binding.native.effective_state
        }
    } else if item.unresolved_targets.iter().any(|target| {
        target.target_id == id && target.state == AgentCatalogUnresolvedState::Suspended
    }) {
        AgentAssetState::Disabled
    } else {
        AgentAssetState::Unknown
    };
    if toggle_action_is_relevant(state, kind) {
        return None;
    }
    Some(
        match state {
            AgentAssetState::Enabled => "此配置已启用，无需再次启用",
            AgentAssetState::Disabled => "此配置已停用，无需再次停用",
            AgentAssetState::Shadowed => "此入口已被其他配置覆盖，请管理当前生效的来源",
            AgentAssetState::NotInstalled => "此配置尚未安装，无法切换启停状态",
            AgentAssetState::Blocked => "此配置受原生策略限制，请先查看来源与诊断",
            AgentAssetState::Invalid => "此配置无效，请先查看并修正原生配置",
            AgentAssetState::Unknown => "当前启停状态无法确认，请先核对配置来源",
        }
        .to_owned(),
    )
}

pub(super) fn action_owner<'a>(
    snapshot: &'a MutationInventory,
    binding: &'a AgentCatalogBinding,
    action: AgentAssetActionKind,
) -> &'a AgentAssetRecord {
    if binding
        .native
        .actions
        .iter()
        .any(|candidate| candidate.action == action && candidate.available)
    {
        return &binding.native;
    }
    binding
        .native
        .relationships
        .action_owner
        .as_ref()
        .and_then(|owner| {
            snapshot
                .inventory
                .assets
                .iter()
                .find(|asset| &asset.stable_id == owner)
        })
        .unwrap_or(&binding.native)
}
