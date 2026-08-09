use crate::{
    models::{
        AppData, AppSettings, CliConfigFile, CliConfigPreview, CliEnvironmentProbeResult,
        CliRuntimeSnapshot, CliSessionSummary, LivenessCliKind, Provider, TemporaryCliInstance,
        TemporaryCliLaunchInput, TemporaryCliLaunchPreview, TemporaryCliLaunchResult,
        TemporaryCliPreference, TemporaryCliSessionMode, Workspace, WorkspaceDirectoryListing,
    },
    services::{self, provider_service::ProviderService},
    state::AppState,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use super::run_blocking;

struct PreparedTemporaryCliLaunch {
    data: AppData,
    provider: Provider,
    settings: AppSettings,
    input: TemporaryCliLaunchInput,
    cli_path: String,
    cli_kind: LivenessCliKind,
    workdir: PathBuf,
    api_key: String,
    model: String,
    preference_model: String,
}

fn prepare_temporary_cli_launch(
    app: &AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<PreparedTemporaryCliLaunch, String> {
    let data = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let provider = data
        .providers
        .iter()
        .find(|provider| provider.identity.id == input.provider_id)
        .cloned()
        .ok_or_else(|| "中转站不存在".to_string())?;
    let cli_kind = input.cli_kind;
    let session_mode = input.session_mode;
    let api_key = if input.api_key.trim().is_empty() {
        provider.auth.api_key.trim().to_string()
    } else {
        input.api_key.trim().to_string()
    };
    if api_key.is_empty() {
        return Err("缺少 API Key，无法启动临时 CLI".to_string());
    }
    let saved_preference = data
        .temporary_cli_preferences
        .iter()
        .find(|preference| preference.provider_id == provider.identity.id);
    let saved_model = saved_preference
        .map(|preference| preference.model.trim().to_string())
        .unwrap_or_default();
    let model = match session_mode {
        TemporaryCliSessionMode::New => [
            input.model.trim(),
            saved_preference
                .map(|preference| preference.model.trim())
                .unwrap_or_default(),
            provider.cli.preferred_model.trim(),
            provider.liveness.model.trim(),
            data.settings.liveness_model.trim(),
        ]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string(),
        TemporaryCliSessionMode::History => input.model.trim().to_string(),
    };
    // 恢复会话的显式模型只影响本次启动，不覆盖新会话的默认模型偏好。
    let preference_model = match session_mode {
        TemporaryCliSessionMode::New if !model.is_empty() => model.clone(),
        _ => saved_model,
    };

    let cli = match cli_kind {
        LivenessCliKind::Codex => {
            services::liveness::LivenessRunner::find_codex_cli(&data.settings.codex_cli_path)?
        }
        LivenessCliKind::ClaudeCode => {
            services::liveness::LivenessRunner::find_claude_cli(&data.settings.claude_cli_path)?
        }
    };
    let terminal = services::temporary_cli::probe_terminal(input.terminal_kind);
    if !terminal.available {
        let detail = terminal.message.trim();
        return Err(if detail.is_empty() {
            "所选终端当前不可用，请重新扫描终端".to_string()
        } else {
            format!("所选终端当前不可用，请重新扫描终端：{detail}")
        });
    }
    let mut settings = data.settings.clone();
    settings.temporary_cli_terminal_kind = input.terminal_kind;
    match cli_kind {
        LivenessCliKind::Codex => settings.codex_cli_path = cli.path.clone(),
        LivenessCliKind::ClaudeCode => settings.claude_cli_path = cli.path.clone(),
    }
    let workdir = services::workspaces::normalize_directory(std::path::Path::new(&input.workdir))?;
    Ok(PreparedTemporaryCliLaunch {
        data,
        provider,
        settings,
        input,
        cli_path: cli.path,
        cli_kind,
        workdir,
        api_key,
        model,
        preference_model,
    })
}

fn launch_options<'a>(
    prepared: &'a PreparedTemporaryCliLaunch,
) -> services::temporary_cli::LaunchOptions<'a> {
    services::temporary_cli::LaunchOptions {
        api_key_override: &prepared.api_key,
        model_override: &prepared.model,
        session_name_override: &prepared.input.session_name,
        resume_id: &prepared.input.resume_id,
        session_mode: prepared.input.session_mode,
    }
}

#[tauri::command]
pub(crate) async fn launch_temporary_cli(
    app: AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<TemporaryCliLaunchResult, String> {
    // The launch path performs process probes, filesystem writes and terminal
    // activation. Keep all of that work off both the UI and async worker pools.
    run_blocking("启动临时 CLI", move || {
        launch_temporary_cli_blocking(app, input)
    })
    .await
}

fn launch_temporary_cli_blocking(
    app: AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<TemporaryCliLaunchResult, String> {
    let prepared = prepare_temporary_cli_launch(&app, input)?;
    let instance = services::temporary_cli::launch(
        &prepared.settings,
        &prepared.provider,
        prepared.cli_kind,
        &prepared.workdir,
        launch_options(&prepared),
    )?;
    let fallback_preference = TemporaryCliPreference {
        provider_id: prepared.provider.identity.id.clone(),
        cli_kind: prepared.cli_kind,
        api_key_token_id: prepared.input.api_key_token_id.trim().to_string(),
        model: prepared.preference_model.clone(),
        workspace_path: prepared.workdir.to_string_lossy().to_string(),
    };
    let (workspaces, workspace_error, preference) = match ProviderService::new(&app)
        .record_temporary_cli_launch(
            &prepared.provider.identity.id,
            prepared.cli_kind,
            &prepared.cli_path,
            &prepared.workdir,
            &prepared.input.api_key_token_id,
            &prepared.preference_model,
        ) {
        Ok((workspaces, preference)) => (workspaces, None, preference),
        Err(err) => (
            prepared.data.workspaces.clone(),
            Some(err),
            fallback_preference,
        ),
    };
    Ok(TemporaryCliLaunchResult {
        instance,
        workspaces,
        workspace_error,
        preference,
    })
}

#[tauri::command]
pub(crate) async fn preview_temporary_cli_launch(
    app: AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<TemporaryCliLaunchPreview, String> {
    run_blocking("生成临时 CLI 启动预览", move || {
        let prepared = prepare_temporary_cli_launch(&app, input)?;
        services::temporary_cli::preview(
            &prepared.settings,
            &prepared.provider,
            prepared.cli_kind,
            &prepared.workdir,
            launch_options(&prepared),
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_cli_sessions(
    cli_kind: LivenessCliKind,
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
        let providers = app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .providers
            .clone();
        Ok(services::cli_runtime::snapshot(&providers))
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
    cli_kind: LivenessCliKind,
) -> Result<CliConfigPreview, String> {
    run_blocking("读取 CLI 配置预览", move || {
        let data = app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let provider = data
            .providers
            .iter()
            .find(|provider| provider.identity.id == id)
            .cloned()
            .ok_or_else(|| "中转站不存在".to_string())?;

        services::cli_runtime::preview_config(&provider, cli_kind)
    })
    .await
}

#[tauri::command]
pub(crate) async fn switch_cli_config(
    app: AppHandle,
    id: String,
    cli_kind: LivenessCliKind,
    revision: String,
    files: Vec<CliConfigFile>,
) -> Result<CliRuntimeSnapshot, String> {
    run_blocking("切换 CLI 配置", move || {
        let data = app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let provider = data
            .providers
            .iter()
            .find(|provider| provider.identity.id == id)
            .cloned()
            .ok_or_else(|| "中转站不存在".to_string())?;

        services::cli_runtime::switch_config(&provider, cli_kind, Some(&revision), &files)?;
        Ok(services::cli_runtime::snapshot(&data.providers))
    })
    .await
}

#[tauri::command]
pub(crate) async fn probe_cli_environment(
    app: AppHandle,
) -> Result<CliEnvironmentProbeResult, String> {
    run_blocking("探测 CLI 环境", move || {
        ProviderService::new(&app).probe_cli_environment()
    })
    .await
}
