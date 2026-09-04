use crate::{
    models::{
        AgentAssetOpenTarget, AgentAssetReadResult, AgentCliKind, AgentEnvironmentInventory,
        AgentHookInspection, AgentHookMutation, AgentHookPlan, AgentVersionCheckResult,
        CliConfigFile, CliConfigPreview, CliEnvironmentProbeResult, CliRuntimeSnapshot,
        CliSessionDetail, CliSessionIndexStatus, CliSessionSearchResponse, TemporaryCliInstance,
        TemporaryCliLaunchInput, TemporaryCliLaunchPreview, TemporaryCliLaunchResult,
        TerminalEnvironmentProbeResult, Workspace, WorkspaceDirectoryListing,
    },
    services::{
        self,
        agent_runtime::{repository::AgentRuntimeSnapshot, service::AgentRuntimeServiceStatus},
        provider_service::ProviderService,
    },
    state::AppState,
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

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
pub(crate) async fn get_agent_runtime_snapshot(
    app: AppHandle,
) -> Result<AgentRuntimeSnapshot, String> {
    run_blocking("读取 Agent runtime 快照", move || {
        app.state::<services::agent_runtime::service::AgentRuntimeService>()
            .snapshot()
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_agent_runtime_status(
    app: AppHandle,
) -> Result<AgentRuntimeServiceStatus, String> {
    run_blocking("读取 Agent runtime 状态", move || {
        Ok(app
            .state::<services::agent_runtime::service::AgentRuntimeService>()
            .status())
    })
    .await
}

#[tauri::command]
pub(crate) async fn activate_agent_runtime(
    app: AppHandle,
    runtime_id: String,
) -> Result<(), String> {
    run_blocking("激活 Agent runtime 终端", move || {
        app.state::<services::agent_runtime::service::AgentRuntimeService>()
            .activate_runtime(&runtime_id)
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
pub(crate) async fn get_agent_environment_inventory(
    app: AppHandle,
    workspace: Option<String>,
) -> Result<AgentEnvironmentInventory, String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    run_blocking("盘点 Agent 环境", move || {
        let workspace_path = workspace.as_deref().map(std::path::Path::new);
        if workspace_path.is_some_and(|path| !path.is_absolute()) {
            return Err("工作区路径必须是绝对路径".to_string());
        }
        services::agent_cli::environment_inventory(&settings, workspace_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn check_agent_latest_versions(
    app: AppHandle,
    workspace: Option<String>,
) -> Result<AgentVersionCheckResult, String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let inventory = run_blocking("盘点 Agent 版本", {
        let settings = settings.clone();
        move || {
            let workspace_path = workspace.as_deref().map(std::path::Path::new);
            if workspace_path.is_some_and(|path| !path.is_absolute()) {
                return Err("工作区路径必须是绝对路径".to_string());
            }
            services::agent_cli::environment_inventory(&settings, workspace_path)
        }
    })
    .await?;
    services::agent_cli::environment::check_latest_versions(&settings, inventory).await
}

#[tauri::command]
pub(crate) async fn read_agent_environment_asset(
    asset_id: String,
    workspace: Option<String>,
) -> Result<AgentAssetReadResult, String> {
    run_blocking("读取 Agent 资产预览", move || {
        let workspace_path = workspace.as_deref().map(std::path::Path::new);
        if workspace_path.is_some_and(|path| !path.is_absolute()) {
            return Err("工作区路径必须是绝对路径".to_string());
        }
        services::agent_cli::read_environment_asset(&asset_id, workspace_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn open_agent_environment_asset(
    app: AppHandle,
    asset_id: String,
    workspace: Option<String>,
    target: AgentAssetOpenTarget,
) -> Result<(), String> {
    run_blocking("打开 Agent 资产", move || {
        let workspace_path = workspace.as_deref().map(std::path::Path::new);
        if workspace_path.is_some_and(|path| !path.is_absolute()) {
            return Err("工作区路径必须是绝对路径".to_string());
        }
        let path = services::agent_cli::environment_asset_path(&asset_id, workspace_path, target)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|error| format!("无法打开 Agent 资产: {error}"))
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

#[tauri::command]
pub(crate) async fn inspect_agent_hook(
    app: AppHandle,
    agent_kind: AgentCliKind,
) -> Result<AgentHookInspection, String> {
    run_blocking("检查 Agent Hook", move || {
        let service = services::agent_runtime::managed_hook::app_service(&app, agent_kind)?;
        let mut inspection = services::agent_runtime::managed_hook::inspect(&service);
        gate_hook_inspection_for_installation(
            &mut inspection,
            agent_hook_installation_available(&app, agent_kind),
        );
        Ok(inspection)
    })
    .await
}

#[tauri::command]
pub(crate) async fn plan_agent_hook(
    app: AppHandle,
    agent_kind: AgentCliKind,
    mutation: AgentHookMutation,
) -> Result<AgentHookPlan, String> {
    run_blocking("生成 Agent Hook 变更计划", move || {
        let service = services::agent_runtime::managed_hook::app_service(&app, agent_kind)?;
        let mut plan = services::agent_runtime::managed_hook::plan(&service, mutation);
        gate_hook_plan_for_installation(
            &mut plan,
            agent_hook_installation_available(&app, agent_kind),
        );
        Ok(plan)
    })
    .await
}

#[tauri::command]
pub(crate) async fn apply_agent_hook(
    app: AppHandle,
    agent_kind: AgentCliKind,
    plan: AgentHookPlan,
) -> Result<AgentHookInspection, String> {
    run_blocking("应用 Agent Hook 变更", move || {
        let installation_available = agent_hook_installation_available(&app, agent_kind);
        ensure_hook_mutation_installation(plan.mutation, installation_available, agent_kind)?;
        let service = services::agent_runtime::managed_hook::app_service(&app, agent_kind)?;
        let mut inspection = services::agent_runtime::managed_hook::apply(&service, plan)?;
        gate_hook_inspection_for_installation(&mut inspection, installation_available);
        Ok(inspection)
    })
    .await
}

#[tauri::command]
pub(crate) async fn health_agent_hook(
    app: AppHandle,
    agent_kind: AgentCliKind,
) -> Result<AgentHookInspection, String> {
    run_blocking("读取 Agent Hook 健康状态", move || {
        let service = services::agent_runtime::managed_hook::app_service(&app, agent_kind)?;
        let mut inspection = services::agent_runtime::managed_hook::inspect(&service);
        gate_hook_inspection_for_installation(
            &mut inspection,
            agent_hook_installation_available(&app, agent_kind),
        );
        Ok(inspection)
    })
    .await
}

#[tauri::command]
pub(crate) async fn repair_agent_hook(
    app: AppHandle,
    agent_kind: AgentCliKind,
) -> Result<AgentHookPlan, String> {
    run_blocking("生成 Agent Hook 修复计划", move || {
        let service = services::agent_runtime::managed_hook::app_service(&app, agent_kind)?;
        let mut plan =
            services::agent_runtime::managed_hook::plan(&service, AgentHookMutation::Install);
        gate_hook_plan_for_installation(
            &mut plan,
            agent_hook_installation_available(&app, agent_kind),
        );
        Ok(plan)
    })
    .await
}

#[tauri::command]
pub(crate) async fn verify_agent_hook(
    app: AppHandle,
    agent_kind: AgentCliKind,
) -> Result<AgentHookInspection, String> {
    run_blocking("验证 Agent Hook", move || {
        let service = services::agent_runtime::managed_hook::app_service(&app, agent_kind)?;
        let mut inspection = services::agent_runtime::managed_hook::inspect(&service);
        gate_hook_inspection_for_installation(
            &mut inspection,
            agent_hook_installation_available(&app, agent_kind),
        );
        Ok(inspection)
    })
    .await
}

fn agent_hook_installation_available(app: &AppHandle, agent_kind: AgentCliKind) -> bool {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    services::agent_cli::find(&settings, agent_kind, false).is_ok()
}

fn hook_mutation_requires_installation(mutation: AgentHookMutation) -> bool {
    matches!(
        mutation,
        AgentHookMutation::Install | AgentHookMutation::Enable
    )
}

fn gate_hook_inspection_for_installation(
    inspection: &mut AgentHookInspection,
    installation_available: bool,
) {
    if !installation_available
        && !inspection.installed
        && inspection.state == crate::models::AgentHookHealthState::NotInstalled
    {
        inspection.state = crate::models::AgentHookHealthState::Unsupported;
        inspection
            .diagnostics
            .push("未检测到 Agent CLI，安装后才能创建 Hook".to_string());
    }
    inspection.set_actions(installation_available);
}

fn gate_hook_plan_for_installation(plan: &mut AgentHookPlan, installation_available: bool) {
    if hook_mutation_requires_installation(plan.mutation) && !installation_available {
        plan.supported = false;
        plan.changes.clear();
        plan.summary = "未检测到 Agent CLI，不能创建或启用 Hook".to_string();
    }
}

fn ensure_hook_mutation_installation(
    mutation: AgentHookMutation,
    installation_available: bool,
    agent_kind: AgentCliKind,
) -> Result<(), String> {
    if hook_mutation_requires_installation(mutation) && !installation_available {
        return Err(format!(
            "未检测到 {} CLI，未修改 Hook 配置",
            services::agent_cli::definition(agent_kind).label
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentHookChange, AgentHookChangeKind, AgentRuntimeScope};

    fn hook_plan(mutation: AgentHookMutation) -> AgentHookPlan {
        AgentHookPlan {
            agent_kind: AgentCliKind::Codex,
            mutation,
            runtime_scope: AgentRuntimeScope::Native,
            config_path: "/tmp/hooks.json".to_string(),
            expected_revision: "revision".to_string(),
            supported: true,
            conflict: false,
            changes: vec![AgentHookChange {
                event_name: "SessionStart".to_string(),
                structural_identity: "balancehub:test".to_string(),
                fingerprint: "fingerprint".to_string(),
                kind: AgentHookChangeKind::Add,
            }],
            summary: "安装".to_string(),
        }
    }

    #[test]
    fn unavailable_agent_blocks_only_installing_or_enabling_hooks() {
        for mutation in [AgentHookMutation::Install, AgentHookMutation::Enable] {
            let mut plan = hook_plan(mutation);
            gate_hook_plan_for_installation(&mut plan, false);
            assert!(!plan.supported);
            assert!(plan.changes.is_empty());
            assert!(
                ensure_hook_mutation_installation(mutation, false, AgentCliKind::Codex).is_err()
            );
        }
        for mutation in [AgentHookMutation::Disable, AgentHookMutation::Remove] {
            let mut plan = hook_plan(mutation);
            gate_hook_plan_for_installation(&mut plan, false);
            assert!(plan.supported);
            assert_eq!(plan.changes.len(), 1);
            assert!(
                ensure_hook_mutation_installation(mutation, false, AgentCliKind::Codex).is_ok()
            );
        }
    }

    #[test]
    fn unavailable_agent_is_unsupported_until_an_owned_hook_needs_cleanup() {
        let service_root = std::env::temp_dir().join(format!(
            "balancehub-hook-command-test-{}",
            std::process::id()
        ));
        let service = crate::services::agent_runtime::managed_hook::CodexHookService::new(
            service_root.join("hooks.json"),
            service_root.join("ownership.json"),
            service_root.join("helper"),
            service_root,
        );
        let mut inspection = service.inspect();
        gate_hook_inspection_for_installation(&mut inspection, false);
        assert_eq!(
            inspection.state,
            crate::models::AgentHookHealthState::Unsupported
        );
        assert!(inspection
            .diagnostics
            .iter()
            .any(|item| item.contains("未检测到 Agent CLI")));

        inspection.installed = true;
        inspection.state = crate::models::AgentHookHealthState::Disabled;
        gate_hook_inspection_for_installation(&mut inspection, false);
        assert_eq!(
            inspection.state,
            crate::models::AgentHookHealthState::Disabled
        );
        inspection.state = crate::models::AgentHookHealthState::HelperMissing;
        gate_hook_inspection_for_installation(&mut inspection, false);
        assert!(
            !inspection
                .actions
                .iter()
                .find(|action| action.action == crate::models::AgentHookActionKind::Repair)
                .unwrap()
                .available
        );
        gate_hook_inspection_for_installation(&mut inspection, true);
        assert!(
            inspection
                .actions
                .iter()
                .find(|action| action.action == crate::models::AgentHookActionKind::Repair)
                .unwrap()
                .available
        );

        inspection.installed = false;
        inspection.state = crate::models::AgentHookHealthState::Conflict;
        gate_hook_inspection_for_installation(&mut inspection, false);
        assert_eq!(
            inspection.state,
            crate::models::AgentHookHealthState::Conflict
        );
    }
}
