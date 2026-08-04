use crate::{
    models::{
        CliConfigPreview, CliEnvironmentProbeResult, CliRuntimeSnapshot, CliSessionSummary,
        LivenessCliKind, TemporaryCliInstance, TemporaryCliLaunchInput, TemporaryCliLaunchResult,
        TemporaryCliPreference, Workspace, WorkspaceDirectoryListing,
    },
    services::{self, provider_service::ProviderService},
    state::AppState,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) fn launch_temporary_cli(
    app: AppHandle,
    input: TemporaryCliLaunchInput,
) -> Result<TemporaryCliLaunchResult, String> {
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
    let resume_id = input
        .resume_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
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
    let model = if resume_id.is_some() && input.model.trim().is_empty() {
        String::new()
    } else {
        [
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
        .to_string()
    };
    // 未记录模型的历史会话不应覆盖用户为新会话保存的模型偏好。
    let preference_model = if model.is_empty() {
        saved_model
    } else {
        model.clone()
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
    let mut launch_settings = data.settings.clone();
    launch_settings.temporary_cli_terminal_kind = input.terminal_kind;
    match cli_kind {
        LivenessCliKind::Codex => launch_settings.codex_cli_path = cli.path.clone(),
        LivenessCliKind::ClaudeCode => launch_settings.claude_cli_path = cli.path.clone(),
    }
    let workdir = services::workspaces::normalize_directory(std::path::Path::new(&input.workdir))?;
    if let Some(resume_id) = resume_id.as_deref() {
        services::cli_sessions::ensure_resume_id(cli_kind, &workdir, resume_id)?;
    }
    let instance = services::temporary_cli::launch(
        &launch_settings,
        &provider,
        cli_kind,
        &workdir,
        &api_key,
        &model,
        resume_id.as_deref(),
    )?;
    let fallback_preference = TemporaryCliPreference {
        provider_id: provider.identity.id.clone(),
        cli_kind,
        api_key_token_id: input.api_key_token_id.trim().to_string(),
        model: preference_model.clone(),
        workspace_path: workdir.to_string_lossy().to_string(),
    };
    let (workspaces, workspace_error, preference) = match ProviderService::new(&app)
        .record_temporary_cli_launch(
            &provider.identity.id,
            cli_kind,
            &cli.path,
            &workdir,
            &input.api_key_token_id,
            &preference_model,
        ) {
        Ok((workspaces, preference)) => (workspaces, None, preference),
        Err(err) => (data.workspaces, Some(err), fallback_preference),
    };
    Ok(TemporaryCliLaunchResult {
        instance,
        workspaces,
        workspace_error,
        preference,
    })
}

#[tauri::command]
pub(crate) fn get_cli_runtime_snapshot(app: AppHandle) -> CliRuntimeSnapshot {
    let providers = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .providers
        .clone();
    services::cli_runtime::snapshot(&providers)
}

#[tauri::command]
pub(crate) async fn get_temporary_cli_instances() -> Result<Vec<TemporaryCliInstance>, String> {
    tauri::async_runtime::spawn_blocking(services::cli_runtime::active_instances)
        .await
        .map_err(|err| format!("临时 CLI 状态读取任务异常: {err}"))
}

#[tauri::command]
pub(crate) async fn list_cli_sessions(
    cli_kind: LivenessCliKind,
    workdir: String,
) -> Result<Vec<CliSessionSummary>, String> {
    let workdir = services::workspaces::normalize_directory(std::path::Path::new(&workdir))?;
    tauri::async_runtime::spawn_blocking(move || services::cli_sessions::list(cli_kind, &workdir))
        .await
        .map_err(|err| format!("CLI 历史会话读取任务异常: {err}"))?
}

#[tauri::command]
pub(crate) fn activate_temporary_cli(instance_id: String) -> Result<(), String> {
    services::temporary_cli::activate(&instance_id)
}

#[tauri::command]
pub(crate) fn forget_workspace(app: AppHandle, path: String) -> Result<Vec<Workspace>, String> {
    ProviderService::new(&app).forget_workspace(path)
}

#[tauri::command]
pub(crate) async fn browse_workspace_directories(
    path: Option<String>,
) -> Result<WorkspaceDirectoryListing, String> {
    tauri::async_runtime::spawn_blocking(move || services::workspaces::browse(path.as_deref()))
        .await
        .map_err(|err| format!("工作空间目录读取任务异常: {err}"))?
}

#[tauri::command]
pub(crate) fn preview_cli_config(
    app: AppHandle,
    id: String,
    cli_kind: LivenessCliKind,
) -> Result<CliConfigPreview, String> {
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
}

#[tauri::command]
pub(crate) fn switch_cli_config(
    app: AppHandle,
    id: String,
    cli_kind: LivenessCliKind,
    revision: String,
) -> Result<CliRuntimeSnapshot, String> {
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

    services::cli_runtime::switch_config(&provider, cli_kind, Some(&revision))?;
    Ok(services::cli_runtime::snapshot(&data.providers))
}

#[tauri::command]
pub(crate) async fn probe_cli_environment(
    app: AppHandle,
) -> Result<CliEnvironmentProbeResult, String> {
    tauri::async_runtime::spawn_blocking(move || ProviderService::new(&app).probe_cli_environment())
        .await
        .map_err(|err| format!("CLI 探测任务异常: {err}"))?
}
