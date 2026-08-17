use crate::{
    models::{
        AgentCliKind, CliConfigFile, CliConfigPreview, CliEnvironmentProbeResult,
        CliRuntimeSnapshot, CliSessionSummary, TemporaryCliInstance, TemporaryCliLaunchInput,
        TemporaryCliLaunchPreview, TemporaryCliLaunchResult, TerminalEnvironmentProbeResult,
        Workspace, WorkspaceDirectoryListing,
    },
    services::{self, provider_service::ProviderService},
};
use tauri::AppHandle;

use super::run_blocking;

#[tauri::command]
pub(crate) async fn launch_temporary_cli(
    app: AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<TemporaryCliLaunchResult, String> {
    // The launch path performs process probes, filesystem writes and terminal
    // activation. Keep all of that work off both the UI and async worker pools.
    run_blocking("启动临时 CLI", move || {
        services::temporary_cli::TemporaryCliLaunchService::new(&app).launch(input)
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_temporary_cli_launch(
    app: AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<TemporaryCliLaunchPreview, String> {
    run_blocking("生成临时 CLI 启动预览", move || {
        services::temporary_cli::TemporaryCliLaunchService::new(&app).preview(input)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_cli_sessions(
    cli_kind: AgentCliKind,
    workdir: String,
) -> Result<Vec<CliSessionSummary>, String> {
    run_blocking("读取 CLI 历史会话", move || {
        services::cli_sessions::list(cli_kind, std::path::Path::new(&workdir))
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_cli_runtime_snapshot(app: AppHandle) -> Result<CliRuntimeSnapshot, String> {
    run_blocking("读取 CLI 运行状态", move || {
        Ok(services::cli_runtime::CliRuntimeService::new(&app).snapshot())
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_temporary_cli_instances() -> Result<Vec<TemporaryCliInstance>, String> {
    run_blocking("读取临时 CLI 状态", || {
        Ok(services::cli_runtime::active_instances())
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_temporary_cli_instance(
    instance_id: String,
) -> Result<Option<TemporaryCliInstance>, String> {
    run_blocking("读取临时 CLI 状态", move || {
        services::cli_runtime::instance(&instance_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn activate_temporary_cli(instance_id: String) -> Result<(), String> {
    run_blocking("激活临时 CLI", move || {
        services::temporary_cli::activate(&instance_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn forget_workspace(
    app: AppHandle,
    path: String,
) -> Result<Vec<Workspace>, String> {
    run_blocking("删除工作空间记录", move || {
        ProviderService::new(&app).forget_workspace(path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn browse_workspace_directories(
    path: Option<String>,
) -> Result<WorkspaceDirectoryListing, String> {
    run_blocking("读取工作空间目录", move || {
        services::workspaces::browse(path.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_cli_config(
    app: AppHandle,
    id: String,
    cli_kind: AgentCliKind,
) -> Result<CliConfigPreview, String> {
    run_blocking("读取 CLI 配置预览", move || {
        services::cli_runtime::CliRuntimeService::new(&app).preview_config(&id, cli_kind)
    })
    .await
}

#[tauri::command]
pub(crate) async fn switch_cli_config(
    app: AppHandle,
    id: String,
    cli_kind: AgentCliKind,
    revision: String,
    files: Vec<CliConfigFile>,
) -> Result<CliRuntimeSnapshot, String> {
    run_blocking("切换 CLI 配置", move || {
        services::cli_runtime::CliRuntimeService::new(&app)
            .switch_config(&id, cli_kind, &revision, &files)
    })
    .await
}

#[tauri::command]
pub(crate) async fn probe_cli_tools(
    app: AppHandle,
    deep: bool,
) -> Result<CliEnvironmentProbeResult, String> {
    run_blocking("探测 CLI", move || {
        Ok(services::cli_runtime::CliRuntimeService::new(&app).probe_tools(deep))
    })
    .await
}

#[tauri::command]
pub(crate) async fn probe_terminals() -> Result<TerminalEnvironmentProbeResult, String> {
    run_blocking("探测终端", move || {
        Ok(TerminalEnvironmentProbeResult {
            terminals: services::temporary_cli::probe_available_terminals(),
        })
    })
    .await
}
