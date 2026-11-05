//! 桌面 Agent 命令桥接：把 Tauri 参数转成核心分发调用。
//!
//! 业务实现全部在 `balancehub_agent::api`；这里只负责窗口身份、
//! 进度通道和把结构化错误原样回传。

use balancehub_agent::api::ProgressSink;
use balancehub_agent::models::*;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{ipc::Channel, Window};

macro_rules! bridge {
    ($name:ident { $($key:literal : $arg:ident : $ty:ty),* $(,)? }) => {
        #[tauri::command]
        pub(crate) async fn $name(
            window: Window,
            $($arg: $ty),*
        ) -> Result<Value, Value> {
            balancehub_agent::api::dispatch(
                stringify!($name),
                json!({ $($key: $arg),* }),
                window.label(),
                None,
            )
            .await
        }
    };
}

bridge!(get_agent_environment_inventory { "workspace": workspace: Option<String> });
bridge!(read_agent_environment_asset { "request": request: AgentAssetReadRequest });
bridge!(read_agent_environment_source { "request": request: AgentAssetReadRequest });
bridge!(open_agent_environment_asset { "request": request: AgentAssetOpenRequest });
bridge!(open_agent_environment_source { "request": request: AgentAssetOpenRequest });
bridge!(plan_agent_asset { "request": request: AgentAssetPlanRequest, "requestId": request_id: String, "agentKind": agent_kind: AgentCliKind });
bridge!(apply_agent_asset { "request": request: AgentAssetApplyRequest });
bridge!(get_agent_asset_operation { "operationId": operation_id: String });
bridge!(list_agent_asset_operations {});
bridge!(cancel_agent_asset_operation { "operationId": operation_id: String });
bridge!(verify_agent_asset_operation { "operationId": operation_id: String });
bridge!(get_agent_catalog_revision { "workspace": workspace: Option<String> });
bridge!(has_agent_catalog_changes { "workspace": workspace: Option<String>, "revision": revision: String });
bridge!(cancel_agent_catalog_read { "requestId": request_id: String });
bridge!(get_agent_catalog_definition { "assetId": asset_id: String });
bridge!(save_agent_catalog_definition { "request": request: AgentCatalogSaveRequest });
bridge!(delete_agent_catalog_definition { "request": request: AgentCatalogDeleteRequest });
bridge!(adopt_agent_catalog_asset { "request": request: AgentCatalogAdoptRequest });
bridge!(get_agent_catalog_agent_panel { "request": request: AgentCatalogAgentPanelRequest, "requestId": request_id: String });
bridge!(preview_agent_catalog_relation { "request": request: AgentCatalogRelationPreviewRequest });
bridge!(commit_agent_catalog_relation { "request": request: AgentCatalogRelationCommitRequest });
bridge!(plan_agent_catalog { "request": request: AgentCatalogPlanRequest, "requestId": request_id: String });
bridge!(apply_agent_catalog { "request": request: AgentCatalogApplyRequest });
bridge!(get_agent_catalog_operation { "operationId": operation_id: String });
bridge!(list_agent_catalog_operations {});
bridge!(cancel_agent_catalog_operation { "operationId": operation_id: String });
bridge!(read_agent_mcp_form { "text": text: String, "format": format: AgentConfigurationFormat, "agentKind": agent_kind: Option<AgentCliKind> });
bridge!(render_agent_mcp_form { "input": input: AgentCatalogMcpInput, "originalText": original_text: String, "format": format: AgentConfigurationFormat, "agentKind": agent_kind: Option<AgentCliKind> });
bridge!(launch_temporary_cli { "input": input: TemporaryCliLaunchInput });
bridge!(preview_temporary_cli_launch { "input": input: TemporaryCliLaunchInput });
bridge!(get_cli_runtime_snapshot {});
bridge!(get_agent_runtime_snapshot {});
bridge!(activate_agent_runtime { "runtimeId": runtime_id: String });
bridge!(get_temporary_cli_instance { "instanceId": instance_id: String });
bridge!(forget_workspace { "path": path: String });
bridge!(browse_workspace_directories { "path": path: Option<String> });
bridge!(preview_cli_config { "id": id: String, "cliKind": cli_kind: AgentCliKind, "apiKeyLocalId": api_key_local_id: String });
bridge!(get_cached_cli_tools {});
bridge!(probe_cli_tools { "deep": deep: bool });
bridge!(probe_terminals {});
bridge!(inspect_agent_hook { "agentKind": agent_kind: AgentCliKind });
bridge!(plan_agent_hook { "agentKind": agent_kind: AgentCliKind, "mutation": mutation: AgentHookMutation });
bridge!(apply_agent_hook { "agentKind": agent_kind: AgentCliKind, "plan": plan: AgentHookPlan });
bridge!(health_agent_hook { "agentKind": agent_kind: AgentCliKind });
bridge!(repair_agent_hook { "agentKind": agent_kind: AgentCliKind });
bridge!(verify_agent_hook { "agentKind": agent_kind: AgentCliKind });
bridge!(list_agent_configuration_sources { "request": request: AgentConfigurationListRequest });
bridge!(read_agent_configuration_source { "request": request: AgentConfigurationSourceRequest });
bridge!(begin_agent_configuration_edit { "request": request: AgentConfigurationSourceRequest });
bridge!(read_agent_configuration_form { "request": request: AgentConfigurationFormRequest });
bridge!(begin_agent_resource_edit { "request": request: AgentResourceEditRequest, "requestId": request_id: String });
bridge!(plan_agent_configuration_save { "request": request: AgentConfigurationSaveRequest });
bridge!(apply_agent_configuration_plan { "request": request: AgentConfigurationApplyRequest });
bridge!(discard_agent_configuration_edit { "editId": edit_id: String });
bridge!(get_agent_configuration_operation { "operationId": operation_id: String });
bridge!(list_agent_configuration_operations {});
bridge!(cancel_agent_configuration_operation { "operationId": operation_id: String });
bridge!(open_agent_configuration_source { "request": request: AgentConfigurationOpenRequest });
bridge!(list_agent_instruction_sets {});
bridge!(save_agent_instruction_set { "request": request: AgentInstructionSaveRequest });
bridge!(delete_agent_instruction_set { "id": id: String, "expectedRevision": expected_revision: String });
bridge!(list_agent_instruction_targets { "request": request: AgentConfigurationListRequest });
bridge!(begin_agent_instruction_edit { "request": request: AgentInstructionEditRequest });
bridge!(set_agent_automatic_update { "request": request: SetAgentAutomaticUpdateRequest });
bridge!(get_agent_lifecycle_catalog { "request": request: AgentLifecycleCatalogRequest });
bridge!(plan_agent_lifecycle { "request": request: AgentLifecyclePlanRequest });
bridge!(apply_agent_lifecycle { "request": request: AgentLifecycleApplyRequest });
bridge!(get_agent_lifecycle_operation { "operationId": operation_id: String });
bridge!(list_agent_lifecycle_operations {});
bridge!(cancel_agent_lifecycle_operation { "operationId": operation_id: String });
bridge!(get_agent_maintenance_change { "planToken": plan_token: String, "index": index: usize });
bridge!(discard_agent_maintenance_plan { "planToken": plan_token: String });
bridge!(list_agent_history { "offset": offset: usize, "limit": limit: usize });
bridge!(plan_agent_history_restore { "historyId": history_id: String, "selectedPaths": selected_paths: Option<Vec<String>> });
bridge!(list_agent_shared_versions { "assetId": asset_id: String });
bridge!(plan_agent_shared_version { "assetId": asset_id: String, "version": version: u64 });
bridge!(get_agent_skill_origin { "assetId": asset_id: String, "workspace": workspace: Option<String> });
bridge!(save_agent_skill_origin { "assetId": asset_id: String, "version": version: u64, "origin": origin: Option<AgentSkillOrigin> });
bridge!(check_agent_skill_update { "assetId": asset_id: String, "requestId": request_id: String });
bridge!(plan_agent_skill_update { "assetId": asset_id: String, "requestId": request_id: String });
bridge!(get_agent_batch_options { "assetIds": asset_ids: Vec<String>, "action": action: AgentCatalogAction, "workspace": workspace: Option<String>, "requestId": request_id: String });
bridge!(plan_agent_batch { "requests": requests: Vec<AgentCatalogPlanRequest>, "workspace": workspace: Option<String>, "requestId": request_id: String });
bridge!(apply_agent_maintenance { "request": request: AgentMaintenanceApplyRequest });
bridge!(get_agent_mcp_tool_rules { "request": request: AgentMcpToolsRequest, "requestId": request_id: String });
bridge!(plan_agent_mcp_tools { "request": request: AgentMcpToolsRequest, "disabled": disabled: Vec<String>, "requestId": request_id: String });
bridge!(plan_agent_session_relocation { "request": request: AgentSessionRelocationRequest });
bridge!(get_agent_maintenance_operation { "operationId": operation_id: String });
bridge!(list_agent_maintenance_operations {});
bridge!(cancel_agent_maintenance_operation { "operationId": operation_id: String });
bridge!(start_agent_mcp_check { "connection": connection: String });
bridge!(get_agent_mcp_check { "id": id: String });
bridge!(cancel_agent_mcp_check { "id": id: String });
bridge!(get_cached_agent_overview { "workspace": workspace: Option<String> });
bridge!(refresh_agent_overview { "agentKind": agent_kind: AgentCliKind, "workspace": workspace: Option<String>, "force": force: bool });
bridge!(resume_agent_session { "request": request: AgentSessionResumeRequest });
bridge!(get_agent_session_resume_operation { "operationId": operation_id: String });
bridge!(list_agent_session_resume_operations {});
bridge!(cancel_agent_session_resume_operation { "operationId": operation_id: String });
bridge!(count_agent_sessions { "request": request: AgentSessionCountRequest });
bridge!(get_agent_session_scope { "explicitWorkdir": explicit_workdir: Option<String> });
bridge!(query_agent_sessions { "request": request: AgentSessionQuery });
bridge!(get_agent_session_detail { "request": request: AgentSessionDetailRequest });
bridge!(get_agent_session_message_content { "request": request: AgentSessionMessageRequest });
bridge!(get_agent_model_catalog { "context": context: AgentModelCatalogContext, "refresh": refresh: bool });
bridge!(estimate_session_usage { "usage": usage: SessionUsageSummary });
bridge!(get_temporary_cli_settings { "request": request: TemporaryCliSettingsRequest });
bridge!(get_temporary_cli_assets { "request": request: TemporaryCliSettingsRequest, "refresh": refresh: bool });
bridge!(list_temporary_cli_profiles {});
bridge!(save_temporary_cli_profile { "profile": profile: TemporaryCliProfile });
bridge!(delete_temporary_cli_profile { "id": id: String });
bridge!(get_wsl_inventory { "force": force: bool });
bridge!(start_wsl_distribution { "distroId": distro_id: String });

struct ChannelSink(Channel<Value>);

impl ProgressSink for ChannelSink {
    fn send(&self, event: Value) -> Result<(), String> {
        self.0.send(event).map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub(crate) async fn get_agent_asset_catalog(
    window: Window,
    workspace: Option<String>,
    progress: Channel<Value>,
) -> Result<Value, Value> {
    balancehub_agent::api::dispatch(
        "get_agent_asset_catalog",
        json!({ "workspace": workspace }),
        window.label(),
        Some(Arc::new(ChannelSink(progress))),
    )
    .await
}

#[tauri::command]
pub(crate) async fn read_agent_catalog_content(
    window: Window,
    request: AgentCatalogContentRequest,
    progress: Channel<Value>,
) -> Result<Value, Value> {
    balancehub_agent::api::dispatch(
        "read_agent_catalog_content",
        json!({ "request": request }),
        window.label(),
        Some(Arc::new(ChannelSink(progress))),
    )
    .await
}

#[tauri::command]
pub(crate) async fn cancel_agent_session_query(
    window: Window,
    request: AgentSessionCancelRequest,
) -> Result<Value, Value> {
    balancehub_agent::api::dispatch(
        "cancel_agent_session_query",
        json!({ "request": request }),
        window.label(),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// 桌面必须注册 `balancehub_agent::api` 的全部命令：少一条时浏览器能用、
    /// 桌面却报「命令不存在」。这里直接扫描桥接文件本身，不再维护第二份清单。
    #[test]
    fn every_agent_command_is_bridged() {
        let source = include_str!("agent.rs");
        // 只看测试模块之前的桥接清单，避免把本测试里的字样也算进去。
        let source = source.split("#[cfg(test)]").next().unwrap_or_default();
        let mut bridged: BTreeSet<&str> = source
            .split("bridge!(")
            .skip(1)
            .filter_map(|rest| rest.split_whitespace().next())
            .collect();
        // 带进度通道或只返回空值的命令是手写实现，不经过 bridge! 宏。
        bridged.extend([
            "get_agent_asset_catalog",
            "read_agent_catalog_content",
            "cancel_agent_session_query",
        ]);
        let expected: BTreeSet<&str> = balancehub_agent::api::COMMANDS.iter().copied().collect();
        let missing: Vec<_> = expected.difference(&bridged).collect();
        let extra: Vec<_> = bridged.difference(&expected).collect();
        assert!(missing.is_empty(), "桌面缺少 Agent 命令桥接：{missing:?}");
        assert!(extra.is_empty(), "桌面桥接了不存在的 Agent 命令：{extra:?}");
    }
}
