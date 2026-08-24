use crate::{
    models::{
        AgentCliKind, CliConfigFile, CliConfigPreview, CliEnvironmentProbeResult,
        CliRuntimeSnapshot, CliSessionDetail, CliSessionIndexStatus, CliSessionSearchResponse,
        TemporaryCliInstance, TemporaryCliLaunchInput, TemporaryCliLaunchPreview,
        TemporaryCliLaunchResult, TerminalEnvironmentProbeResult, Workspace,
        WorkspaceDirectoryListing,
    },
    services::{self, provider_service::ProviderService},
    state::AppState,
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Manager};

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

const SESSION_SEARCH_TIMEOUT: Duration = Duration::from_secs(60);
const SESSION_DETAIL_TIMEOUT: Duration = Duration::from_secs(20);
static SESSION_SEARCH_GENERATION: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
pub(crate) async fn search_cli_sessions(
    app: AppHandle,
    cli_kind: AgentCliKind,
    workdir: String,
    query: String,
    limit: Option<usize>,
    force_refresh: Option<bool>,
) -> Result<CliSessionSearchResponse, String> {
    let generation = SESSION_SEARCH_GENERATION
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let task_app = app.clone();
    let task = move || {
        services::cli_sessions::search(
            &task_app,
            &settings,
            cli_kind,
            std::path::Path::new(&workdir),
            services::cli_sessions::SearchOptions {
                query: &query,
                limit: limit.unwrap_or(50),
                force_refresh: force_refresh.unwrap_or(false),
            },
            || SESSION_SEARCH_GENERATION.load(Ordering::Relaxed) == generation,
        )
    };
    match tokio::time::timeout(
        SESSION_SEARCH_TIMEOUT,
        run_blocking("检索 CLI 历史会话", task),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let _ = SESSION_SEARCH_GENERATION.compare_exchange(
                generation,
                generation.wrapping_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            Err("检索 CLI 历史会话超时，请缩小搜索范围后重试".to_string())
        }
    }
}

#[tauri::command]
pub(crate) async fn get_cli_session_index_status(
    app: AppHandle,
) -> Result<CliSessionIndexStatus, String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    run_blocking("读取会话索引状态", move || {
        let config = services::cli_sessions::index_config(&app, &settings)?;
        services::cli_sessions::index_status(&config, settings.session_index_max_size_mib)
    })
    .await
}

#[tauri::command]
pub(crate) async fn clear_cli_session_index(app: AppHandle) -> Result<(), String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    run_blocking("清理会话索引", move || {
        let config = services::cli_sessions::index_config(&app, &settings)?;
        services::cli_sessions::clear_index(&config)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_cli_session_detail(
    cli_kind: AgentCliKind,
    workdir: String,
    session_id: String,
) -> Result<CliSessionDetail, String> {
    match tokio::time::timeout(
        SESSION_DETAIL_TIMEOUT,
        run_blocking("读取 CLI 会话详情", move || {
            services::cli_sessions::detail(cli_kind, std::path::Path::new(&workdir), &session_id)
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err("读取 CLI 会话详情超时，请稍后重试".to_string()),
    }
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
    api_key_local_id: String,
) -> Result<CliConfigPreview, String> {
    run_blocking("读取 CLI 配置预览", move || {
        services::cli_runtime::CliRuntimeService::new(&app).preview_config(
            &id,
            cli_kind,
            &api_key_local_id,
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn switch_cli_config(
    app: AppHandle,
    id: String,
    cli_kind: AgentCliKind,
    api_key_local_id: String,
    revision: String,
    files: Vec<CliConfigFile>,
) -> Result<CliRuntimeSnapshot, String> {
    run_blocking("切换 CLI 配置", move || {
        services::cli_runtime::CliRuntimeService::new(&app).switch_config(
            &id,
            cli_kind,
            &api_key_local_id,
            &revision,
            &files,
        )
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
