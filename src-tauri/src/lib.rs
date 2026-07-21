mod adapters;
mod models;
mod network;
mod providers;
mod services;
mod state;
mod storage;
mod tray;
mod util;

use models::{
    AppData, AppDataTransferResult, AppSettings, CliCandidate, CliConfigPreview,
    CliRuntimeSnapshot, CodexCliProbeResult, CodexModelSyncResult, LivenessCliKind,
    LivenessRunResult, Provider, ProviderApiKeyOption, ProviderCapabilityProbeResult,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderRequestLogsQuery,
    ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderUsageSummary, RefreshResult,
    TemporaryCliInstance, TemporaryCliLaunchInput, TemporaryCliLaunchResult,
    TemporaryCliPreference, Workspace, WorkspaceDirectoryListing,
};
use services::liveness::preview_prompts;
use services::notifications::NotificationSendResult;
use services::provider_service::ProviderService;
use state::AppState;
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};

#[tauri::command]
fn backend_status() -> &'static str {
    "ready"
}

#[tauri::command]
fn host_platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn open_ccswitch_deeplink(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("ccswitch://v1/import?") {
        return Err("无效的 CC Switch 深链".to_string());
    }

    open_deeplink_with_system(trimmed)
}

#[tauri::command]
fn launch_temporary_cli(
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
    let model = [
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
    .to_string();

    let cli = match cli_kind {
        LivenessCliKind::Codex => {
            services::liveness::LivenessRunner::find_codex_cli(&data.settings.codex_cli_path)?
        }
        LivenessCliKind::ClaudeCode => {
            services::liveness::LivenessRunner::find_claude_cli(&data.settings.claude_cli_path)?
        }
    };
    let mut launch_settings = data.settings.clone();
    match cli_kind {
        LivenessCliKind::Codex => launch_settings.codex_cli_path = cli.path.clone(),
        LivenessCliKind::ClaudeCode => launch_settings.claude_cli_path = cli.path.clone(),
    }
    let workdir = services::workspaces::normalize_directory(std::path::Path::new(&input.workdir))?;
    let instance = services::temporary_cli::launch(
        &launch_settings,
        &provider,
        cli_kind,
        &workdir,
        &api_key,
        &model,
    )?;
    let fallback_preference = TemporaryCliPreference {
        provider_id: provider.identity.id.clone(),
        cli_kind,
        api_key_token_id: input.api_key_token_id.trim().to_string(),
        model: model.clone(),
        workspace_path: workdir.to_string_lossy().to_string(),
    };
    let (workspaces, workspace_error, preference) = match ProviderService::new(&app)
        .record_temporary_cli_launch(
            &provider.identity.id,
            cli_kind,
            &cli.path,
            &workdir,
            &input.api_key_token_id,
            &model,
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
fn get_cli_runtime_snapshot(app: AppHandle) -> CliRuntimeSnapshot {
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
async fn get_temporary_cli_instances() -> Result<Vec<TemporaryCliInstance>, String> {
    tauri::async_runtime::spawn_blocking(services::cli_runtime::active_instances)
        .await
        .map_err(|err| format!("临时 CLI 状态读取任务异常: {err}"))
}

#[tauri::command]
fn activate_temporary_cli(instance_id: String) -> Result<(), String> {
    services::temporary_cli::activate(&instance_id)
}

#[tauri::command]
fn forget_workspace(app: AppHandle, path: String) -> Result<Vec<Workspace>, String> {
    ProviderService::new(&app).forget_workspace(path)
}

#[tauri::command]
async fn browse_workspace_directories(
    path: Option<String>,
) -> Result<WorkspaceDirectoryListing, String> {
    tauri::async_runtime::spawn_blocking(move || services::workspaces::browse(path.as_deref()))
        .await
        .map_err(|err| format!("工作空间目录读取任务异常: {err}"))?
}

#[tauri::command]
fn preview_cli_config(
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
fn switch_cli_config(
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

#[cfg(target_os = "macos")]
fn open_deeplink_with_system(url: &str) -> Result<(), String> {
    let output = std::process::Command::new("open")
        .arg("-b")
        .arg("com.ccswitch.desktop")
        .arg(url)
        .output()
        .map_err(|err| format!("无法调用系统打开器: {err}"))?;

    if output.status.success() {
        return Ok(());
    }

    let fallback = std::process::Command::new("open")
        .arg(url)
        .output()
        .map_err(|err| format!("无法调用系统打开器: {err}"))?;

    if fallback.status.success() {
        Ok(())
    } else {
        Err(command_error_message(fallback))
    }
}

#[cfg(target_os = "windows")]
fn open_deeplink_with_system(url: &str) -> Result<(), String> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .output()
        .map_err(|err| format!("无法调用系统打开器: {err}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_error_message(output))
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn open_deeplink_with_system(url: &str) -> Result<(), String> {
    let output = std::process::Command::new("xdg-open")
        .arg(url)
        .output()
        .map_err(|err| format!("无法调用系统打开器: {err}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_error_message(output))
    }
}

fn command_error_message(output: std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        "系统没有成功打开 CC Switch 深链".to_string()
    } else {
        stderr
    }
}

fn updater_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config>
{
    let builder = tauri_plugin_updater::Builder::new();
    match option_env!("TAURI_UPDATER_PUBLIC_KEY")
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        Some(pubkey) => builder.pubkey(pubkey).build(),
        None => builder.build(),
    }
}

#[tauri::command]
fn load_app_data(app: AppHandle) -> Result<AppData, String> {
    let data = ProviderService::new(&app).load_app_data()?;
    tray::update_tooltip(&app, &data.providers);
    Ok(data)
}

#[tauri::command]
fn save_provider(app: AppHandle, input: ProviderInput) -> Result<Vec<Provider>, String> {
    let providers = ProviderService::new(&app).save_provider(input)?;
    tray::refresh_from_state(&app);
    Ok(providers)
}

#[tauri::command]
fn remove_provider(app: AppHandle, id: String) -> Result<Vec<Provider>, String> {
    let providers = ProviderService::new(&app).remove_provider(id)?;
    tray::refresh_from_state(&app);
    Ok(providers)
}

#[tauri::command]
fn reorder_providers(app: AppHandle, ids: Vec<String>) -> Result<Vec<Provider>, String> {
    let providers = ProviderService::new(&app).reorder_providers(ids)?;
    tray::refresh_from_state(&app);
    Ok(providers)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    let settings = ProviderService::new(&app).save_settings(settings)?;
    tray::refresh_from_state(&app);
    Ok(settings)
}

#[tauri::command]
async fn send_app_notification(
    app: AppHandle,
    settings: AppSettings,
    provider: Option<Provider>,
    title: String,
    markdown: String,
    ignore_switch: bool,
) -> Result<NotificationSendResult, String> {
    if let Some(provider) = provider {
        Ok(services::notifications::send_provider_notification(
            &app,
            &settings,
            &provider,
            title,
            markdown,
            ignore_switch,
        )
        .await)
    } else {
        Ok(services::notifications::send_configured_notification(
            &app,
            &settings,
            title,
            markdown,
            ignore_switch,
        )
        .await)
    }
}

#[tauri::command]
fn export_app_data(app: AppHandle, path: String) -> Result<AppDataTransferResult, String> {
    ProviderService::new(&app).export_app_data(path)
}

#[tauri::command]
fn import_app_data(app: AppHandle, path: String) -> Result<AppDataTransferResult, String> {
    let (_data, result) = ProviderService::new(&app).import_app_data(path)?;
    tray::refresh_from_state(&app);
    Ok(result)
}

#[tauri::command]
async fn complete_provider_credentials(
    app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderCredentialCompletionResult, String> {
    ProviderService::new(&app).complete_credentials(input).await
}

#[tauri::command]
async fn test_provider_connection(
    app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderConnectionTestResult, String> {
    ProviderService::new(&app).test_connection(input).await
}

#[tauri::command]
async fn probe_codex_cli(
    app: AppHandle,
    liveness_cli_kind: Option<LivenessCliKind>,
    codex_cli_path: Option<String>,
    claude_cli_path: Option<String>,
) -> Result<CodexCliProbeResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ProviderService::new(&app).probe_codex_cli(
            liveness_cli_kind,
            codex_cli_path,
            claude_cli_path,
        )
    })
    .await
    .map_err(|err| format!("CLI 探测任务异常: {err}"))?
}

#[tauri::command]
fn preview_liveness_command(app: AppHandle, id: String) -> Result<LivenessRunResult, String> {
    ProviderService::new(&app).liveness_command_preview(id)
}

#[tauri::command]
fn preview_liveness_prompts(settings: AppSettings, count: usize) -> Vec<String> {
    preview_prompts(&settings, count)
}

#[tauri::command]
async fn test_liveness(
    app: AppHandle,
    id: String,
    prompt: Option<String>,
    automatic: Option<bool>,
) -> Result<LivenessRunResult, String> {
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        ProviderService::new(&worker_app).test_liveness(id, prompt, automatic.unwrap_or(false))
    })
    .await
    .map_err(|err| format!("测活任务异常: {err}"))??;
    tray::refresh_from_state(&app);
    Ok(result)
}

#[tauri::command]
async fn probe_provider_site(
    app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderSiteProbeResult, String> {
    ProviderService::new(&app).probe_site(input).await
}

#[tauri::command]
async fn list_provider_api_keys(
    app: AppHandle,
    id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    ProviderService::new(&app).list_api_keys(id).await
}

#[tauri::command]
async fn create_provider_api_key(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    ProviderService::new(&app).create_api_key(id, name).await
}

#[tauri::command]
async fn create_provider_api_key_for_input(
    app: AppHandle,
    input: ProviderInput,
    name: String,
) -> Result<ProviderApiKeyOption, String> {
    ProviderService::new(&app)
        .create_api_key_for_input(input, name)
        .await
}

#[tauri::command]
async fn generate_provider_access_token(
    app: AppHandle,
    id: String,
) -> Result<Vec<Provider>, String> {
    let providers = ProviderService::new(&app).generate_access_token(id).await?;
    tray::refresh_from_state(&app);
    Ok(providers)
}

#[tauri::command]
async fn generate_provider_access_token_for_input(
    app: AppHandle,
    input: ProviderInput,
) -> Result<String, String> {
    ProviderService::new(&app)
        .generate_access_token_for_input(input)
        .await
}

#[tauri::command]
async fn delete_provider_api_key(
    app: AppHandle,
    id: String,
    token_id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    ProviderService::new(&app)
        .delete_api_key(id, token_id)
        .await
}

#[tauri::command]
async fn get_provider_usage(
    app: AppHandle,
    id: String,
    period: String,
) -> Result<ProviderUsageSummary, String> {
    ProviderService::new(&app).usage_summary(id, period).await
}

#[tauri::command]
async fn get_provider_request_logs(
    app: AppHandle,
    id: String,
    query: ProviderRequestLogsQuery,
) -> Result<ProviderRequestLogsResult, String> {
    ProviderService::new(&app).request_logs(id, query).await
}

#[tauri::command]
async fn change_provider_password(
    app: AppHandle,
    id: String,
    original_password: String,
    password: String,
) -> Result<String, String> {
    ProviderService::new(&app)
        .change_password(id, original_password, password)
        .await
}

#[tauri::command]
async fn get_provider_check_in_records(
    app: AppHandle,
    id: String,
    month: String,
) -> Result<ProviderCheckInRecordsResult, String> {
    ProviderService::new(&app).check_in_records(id, month).await
}

#[tauri::command]
async fn probe_provider_capabilities(
    app: AppHandle,
    id: String,
) -> Result<ProviderCapabilityProbeResult, String> {
    ProviderService::new(&app).probe_capabilities(id).await
}

#[tauri::command]
async fn sync_codex_models(app: AppHandle, id: String) -> Result<CodexModelSyncResult, String> {
    ProviderService::new(&app).sync_codex_models(id).await
}

#[tauri::command]
async fn get_provider_invite_link(app: AppHandle, id: String) -> Result<String, String> {
    ProviderService::new(&app).invite_link(id).await
}

#[tauri::command]
async fn refresh_all_providers(app: AppHandle) -> Result<RefreshResult, String> {
    let result = ProviderService::new(&app).refresh_all().await?;
    tray::refresh_from_state(&app);
    Ok(result)
}

#[tauri::command]
async fn refresh_providers(app: AppHandle, ids: Vec<String>) -> Result<RefreshResult, String> {
    let result = ProviderService::new(&app).refresh_by_ids(ids).await?;
    tray::refresh_from_state(&app);
    Ok(result)
}

#[tauri::command]
async fn check_in_provider(app: AppHandle, id: String) -> Result<ProviderCheckInResult, String> {
    ProviderService::new(&app).check_in(id).await
}

#[tauri::command]
fn acknowledge_liveness_cost(app: AppHandle) -> Result<AppSettings, String> {
    let settings = ProviderService::new(&app).acknowledge_liveness_cost()?;
    tray::refresh_from_state(&app);
    Ok(settings)
}

#[tauri::command]
fn revoke_liveness_cost(app: AppHandle) -> Result<AppSettings, String> {
    let settings = ProviderService::new(&app).revoke_liveness_cost()?;
    tray::refresh_from_state(&app);
    Ok(settings)
}

#[tauri::command]
async fn check_cli_path(
    app: AppHandle,
    kind: LivenessCliKind,
    path: Option<String>,
) -> Result<CodexCliProbeResult, String> {
    let path = path.unwrap_or_default().trim().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        ProviderService::new(&app).check_cli_path(kind, &path)
    })
    .await
    .map_err(|err| format!("CLI 探测任务异常: {err}"))?
}

#[tauri::command]
async fn list_cli_candidates(
    app: AppHandle,
    kind: LivenessCliKind,
    path: Option<String>,
) -> Result<Vec<CliCandidate>, String> {
    let path = path.unwrap_or_default().trim().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        ProviderService::new(&app).list_cli_candidates(kind, &path)
    })
    .await
    .map_err(|err| format!("CLI 候选扫描任务异常: {err}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 自启动项固定携带 --silent-start 标记启动来源；是否真的静默由
        // launch_at_login_minimized 设置决定。前端每次保存设置调用 enable()
        // 时都会按这里的参数重写系统启动项。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--silent-start"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(updater_plugin())
        .menu(|handle| {
            let app_menu = SubmenuBuilder::new(handle, "BalanceHub")
                .hide_with_text("隐藏 BalanceHub")
                .hide_others_with_text("隐藏其他")
                .show_all_with_text("全部显示")
                .separator()
                .quit_with_text("退出 BalanceHub")
                .build()?;
            let file_menu = SubmenuBuilder::new(handle, "文件")
                .close_window_with_text("关闭窗口")
                .build()?;
            let edit_menu = SubmenuBuilder::new(handle, "编辑")
                .undo_with_text("撤销")
                .redo_with_text("重做")
                .separator()
                .cut_with_text("剪切")
                .copy_with_text("复制")
                .paste_with_text("粘贴")
                .select_all_with_text("全选")
                .build()?;
            #[cfg(target_os = "macos")]
            let view_menu = SubmenuBuilder::new(handle, "显示")
                .fullscreen_with_text("切换全屏")
                .build()?;

            let menu = MenuBuilder::new(handle)
                .item(&app_menu)
                .item(&file_menu)
                .item(&edit_menu);
            #[cfg(target_os = "macos")]
            let menu = menu.item(&view_menu);

            menu.build()
        })
        .setup(|app| {
            let app_state = match storage::load_app_data(app.app_handle()) {
                Ok(data) => AppState::new(data),
                Err(err) => AppState::with_load_error(AppData::default(), Some(err)),
            };
            // 自启动（--silent-start）且用户勾选了“自启后最小化”时静默启动：
            // 窗口在配置里默认不可见，走到 setup 末尾再决定是否显示。
            let silent_start = std::env::args().any(|arg| arg == "--silent-start")
                && app_state
                    .data
                    .read()
                    .unwrap_or_else(|err| err.into_inner())
                    .settings
                    .launch_at_login_minimized;
            app.manage(app_state);

            // 自动刷新 / 签到 / 测活的调度运行在 Rust 后台任务里，独立于窗口存活。
            services::scheduler::start(app.app_handle());

            // 清扫历史残留的临时 CLI 目录/测活隔离 HOME（可能含明文凭据）。
            // 磁盘扫描放后台线程，不阻塞启动。
            std::thread::spawn(services::temporary_cli::cleanup_stale);

            if let Some(window) = app.get_webview_window("main") {
                let app_handle = window.app_handle().clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        tray::hide_main_window(&app_handle);
                    }
                });
            }

            let menu = MenuBuilder::new(app)
                .text("show", "显示窗口")
                .separator()
                .text("quit", "退出")
                .build()?;

            let mut tray_builder = TrayIconBuilder::with_id(tray::MAIN_TRAY_ID)
                .tooltip("BalanceHub")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => tray::show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        tray::show_main_window(tray.app_handle());
                    }
                });

            #[cfg(target_os = "macos")]
            {
                let tray_icon =
                    tauri::image::Image::new(include_bytes!("../icons/tray-template.rgba"), 32, 32);
                tray_builder = tray_builder.icon(tray_icon).icon_as_template(true);
            }

            #[cfg(not(target_os = "macos"))]
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon).icon_as_template(false);
            }

            tray_builder.build(app)?;
            tray::refresh_from_state(app.app_handle());

            if silent_start {
                // 静默启动不显示窗口；macOS 同步隐藏 Dock 图标（事件循环启动前
                // 直接设置，避免 Dock 图标闪现）。
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            } else {
                tray::show_main_window(app.app_handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            backend_status,
            host_platform,
            open_ccswitch_deeplink,
            launch_temporary_cli,
            get_cli_runtime_snapshot,
            get_temporary_cli_instances,
            activate_temporary_cli,
            forget_workspace,
            browse_workspace_directories,
            preview_cli_config,
            switch_cli_config,
            load_app_data,
            save_provider,
            remove_provider,
            reorder_providers,
            save_settings,
            send_app_notification,
            export_app_data,
            import_app_data,
            complete_provider_credentials,
            test_provider_connection,
            probe_codex_cli,
            preview_liveness_command,
            preview_liveness_prompts,
            test_liveness,
            probe_provider_site,
            list_provider_api_keys,
            create_provider_api_key,
            create_provider_api_key_for_input,
            generate_provider_access_token,
            generate_provider_access_token_for_input,
            delete_provider_api_key,
            get_provider_usage,
            get_provider_request_logs,
            change_provider_password,
            get_provider_check_in_records,
            probe_provider_capabilities,
            sync_codex_models,
            get_provider_invite_link,
            refresh_all_providers,
            refresh_providers,
            check_in_provider,
            acknowledge_liveness_cost,
            revoke_liveness_cost,
            check_cli_path,
            list_cli_candidates
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
