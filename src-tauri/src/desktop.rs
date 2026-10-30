use crate::{
    commands::{
        agent_assets::*, agent_catalog::*, agent_configuration::*, agent_instructions::*,
        agent_lifecycle::*, agent_maintenance::*, agent_mcp_check::*, agent_overview::*,
        agent_session_resume::*, agent_sessions::*, agent_wsl::*, app::*, browser_runtime::*,
        cli::*, cloud_sync::*, login_accounts::*, model_catalog::*, oauth_accounts::*, provider::*,
        provider_app_import::*, provider_browser_login::*, temporary_cli_settings::*,
    },
    models::{AppData, TrayClickAction, WindowCloseAction},
    services::{self, app_updater::AppUpdaterState, provider_service::ProviderService},
    state::AppState,
    storage, tray,
};
use std::path::PathBuf;
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, WindowEvent,
};

pub(crate) fn run() {
    tauri::Builder::default()
        // 必须是第一个插件：在 setup 和后台调度启动前拦截第二个实例。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .manage(AppUpdaterState::default())
        .manage(tray::TrayAvailability::default())
        .manage(services::notifications::NotificationThrottle::default())
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
        .plugin(window_state_plugin())
        .plugin(updater_plugin())
        .menu(build_app_menu)
        .setup(setup_app)
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                services::cli_sessions::workbench::release_window(
                    window.app_handle(),
                    window.label(),
                );
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.clear_asset_actor(window.label());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            cloud_sync_state,
            cloud_sync_save,
            cloud_sync_start,
            cloud_sync_test,
            cloud_sync_confirm,
            cloud_sync_restore,
            cloud_sync_cancel,
            cloud_sync_compare,
            host_platform,
            install_dev_sample_data,
            open_project_repository,
            open_log_directory,
            open_data_directory,
            launch_temporary_cli,
            preview_temporary_cli_launch,
            get_temporary_cli_settings,
            get_agent_model_catalog,
            get_temporary_cli_assets,
            list_temporary_cli_profiles,
            save_temporary_cli_profile,
            delete_temporary_cli_profile,
            get_agent_session_scope,
            query_agent_sessions,
            get_agent_session_detail,
            get_agent_session_message_content,
            cancel_agent_session_query,
            resume_agent_session,
            get_agent_session_resume_operation,
            list_agent_session_resume_operations,
            cancel_agent_session_resume_operation,
            get_cli_session_index_status,
            clear_cli_session_index,
            get_cli_runtime_snapshot,
            get_agent_runtime_snapshot,
            activate_agent_runtime,
            get_temporary_cli_instance,
            forget_workspace,
            browse_workspace_directories,
            preview_cli_config,
            list_agent_configuration_sources,
            list_agent_instruction_sets,
            save_agent_instruction_set,
            delete_agent_instruction_set,
            list_agent_instruction_targets,
            begin_agent_instruction_edit,
            get_wsl_inventory,
            start_wsl_distribution,
            start_agent_mcp_check,
            get_agent_mcp_check,
            cancel_agent_mcp_check,
            read_agent_configuration_source,
            begin_agent_configuration_edit,
            read_agent_configuration_form,
            begin_agent_resource_edit,
            open_external_link,
            plan_agent_configuration_save,
            apply_agent_configuration_plan,
            discard_agent_configuration_edit,
            get_agent_configuration_operation,
            list_agent_configuration_operations,
            cancel_agent_configuration_operation,
            open_agent_configuration_source,
            load_app_data,
            save_provider,
            set_provider_enabled,
            preview_provider_check_in_policy,
            remove_provider,
            reorder_providers,
            save_settings,
            send_app_notification,
            export_app_data,
            import_app_data,
            complete_provider_credentials,
            test_provider_connection,
            probe_cli_tools,
            get_cached_cli_tools,
            get_cached_agent_overview,
            count_agent_sessions,
            refresh_agent_overview,
            inspect_agent_hook,
            plan_agent_hook,
            apply_agent_hook,
            health_agent_hook,
            repair_agent_hook,
            verify_agent_hook,
            get_agent_environment_inventory,
            read_agent_environment_asset,
            open_agent_environment_asset,
            read_agent_environment_source,
            open_agent_environment_source,
            plan_agent_asset,
            apply_agent_asset,
            get_agent_asset_operation,
            cancel_agent_asset_operation,
            list_agent_asset_operations,
            verify_agent_asset_operation,
            get_agent_asset_catalog,
            get_agent_catalog_revision,
            has_agent_catalog_changes,
            read_agent_catalog_content,
            cancel_agent_catalog_read,
            get_agent_catalog_definition,
            list_agent_history,
            get_agent_maintenance_change,
            discard_agent_maintenance_plan,
            get_agent_mcp_tool_rules,
            plan_agent_mcp_tools,
            plan_agent_session_relocation,
            list_agent_shared_versions,
            plan_agent_shared_version,
            get_agent_skill_origin,
            save_agent_skill_origin,
            check_agent_skill_update,
            plan_agent_skill_update,
            get_agent_batch_options,
            plan_agent_batch,
            plan_agent_history_restore,
            apply_agent_maintenance,
            get_agent_maintenance_operation,
            list_agent_maintenance_operations,
            cancel_agent_maintenance_operation,
            read_agent_mcp_form,
            render_agent_mcp_form,
            save_agent_catalog_definition,
            delete_agent_catalog_definition,
            adopt_agent_catalog_asset,
            get_agent_catalog_agent_panel,
            preview_agent_catalog_relation,
            commit_agent_catalog_relation,
            plan_agent_catalog,
            apply_agent_catalog,
            get_agent_catalog_operation,
            list_agent_catalog_operations,
            cancel_agent_catalog_operation,
            get_agent_lifecycle_catalog,
            plan_agent_lifecycle,
            apply_agent_lifecycle,
            get_agent_lifecycle_operation,
            list_agent_lifecycle_operations,
            cancel_agent_lifecycle_operation,
            set_agent_automatic_update,
            probe_terminals,
            preview_liveness_prompts,
            detect_provider_protocol,
            probe_provider_site,
            list_provider_api_keys,
            get_provider_api_key_editor_context,
            get_provider_api_key_editor_context_for_input,
            update_provider_api_key,
            list_local_provider_api_keys,
            add_local_provider_api_key,
            set_local_provider_api_key_remark,
            set_default_local_provider_api_key,
            remove_local_provider_api_key,
            create_provider_api_key,
            create_provider_api_key_for_input,
            generate_provider_access_token_for_input,
            delete_provider_api_key,
            get_provider_usage,
            get_usage_overview,
            get_provider_request_logs,
            change_provider_password,
            get_provider_check_in_records,
            probe_provider_capabilities,
            sync_available_models,
            get_model_metadata,
            estimate_session_usage,
            get_provider_invite_link,
            get_site_announcements,
            mark_site_announcement_read,
            refresh_all_providers_with_progress,
            refresh_providers,
            check_in_provider,
            open_provider_site,
            open_provider_app_import,
            list_check_in_tasks,
            cancel_check_in_task,
            resume_check_in_task,
            show_check_in_window,
            check_in_all_providers,
            start_provider_browser_login,
            list_provider_browser_logins,
            cancel_provider_browser_login,
            show_provider_login_window,
            list_login_accounts,
            create_login_account,
            update_login_account,
            remove_login_account,
            open_login_account,
            list_login_account_cookies,
            read_login_account_cookie,
            start_oauth_login,
            list_oauth_logins,
            cancel_oauth_login,
            submit_oauth_callback,
            retry_oauth_login_save,
            open_oauth_login_page,
            preview_oauth_import,
            discard_oauth_import,
            apply_oauth_import,
            get_oauth_account,
            set_oauth_renewal,
            update_oauth_account,
            get_oauth_credential,
            replace_oauth_credential,
            export_oauth_account,
            preview_cpa_oauth_import,
            send_oauth_to_cpa,
            set_cpa_oauth_disabled,
            update_cpa_oauth_fields,
            refresh_cpa_oauth,
            reset_cpa_oauth_cooldown,
            refresh_all_cpa_oauth,
            list_cpa_error_logs,
            read_cpa_error_log,
            view_cpa_oauth_file,
            delete_cpa_oauth,
            list_cpa_plugins,
            list_cpa_plugin_store,
            install_cpa_plugin,
            check_cpa_version,
            get_cpa_plugin_config,
            set_cpa_plugin_enabled,
            update_cpa_plugin_config,
            remove_cpa_plugin,
            list_cpa_login_providers,
            start_cpa_login,
            get_cpa_login_status,
            submit_cpa_login_callback,
            cancel_cpa_login,
            get_cpa_account_quota,
            open_cpa_login_url,
            get_cpa_model_rules,
            get_cpa_model_definitions,
            update_cpa_model_rules,
            get_cpa_gateway_settings,
            update_cpa_gateway_settings,
            list_cpa_client_keys,
            add_cpa_client_key,
            remove_cpa_client_key,
            read_cpa_request_logs,
            read_cpa_request_log,
            read_cpa_api_key_usage,
            clear_cpa_logs,
            read_cpa_gateway_config,
            update_cpa_gateway_config,
            get_cpa_log_settings,
            set_cpa_log_settings,
            get_provider_credentials,
            read_provider_credential,
            clear_provider_credential,
            validate_provider_credentials,
            get_browser_runtime_status,
            install_browser_runtime,
            select_browser_runtime_browser,
            inspect_browser_runtime_browser,
            cancel_browser_runtime_install,
            uninstall_browser_runtime,
            check_app_update,
            install_app_update,
            cancel_app_update,
            clear_pending_app_update,
            cancel_visible_relaunch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 主窗口的位置/大小由设置决定是否恢复。插件必须先跳过主窗口的自动恢复，
/// 才能在 setup 里按设置决定——插件初始化早于设置加载完成。
fn window_state_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_window_state::Builder::default()
        .with_state_flags(window_state_flags())
        .skip_initial_state("main")
        .build()
}

fn window_state_flags() -> tauri_plugin_window_state::StateFlags {
    // 不恢复可见性：启动可见性是 launch_at_login_minimized 的职责，
    // 让上一次退出时的隐藏状态覆盖它会和「启动时最小化」互相打脸。
    tauri_plugin_window_state::StateFlags::all() & !tauri_plugin_window_state::StateFlags::VISIBLE
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

fn build_app_menu<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
) -> tauri::Result<tauri::menu::Menu<R>> {
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
}

fn setup_app(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // 开发版的配置与数据始终落在临时根目录，不碰安装版目录。
    let config_root =
        config_dir(app.app_handle()).map_err(|_| "无法获取应用配置目录".to_owned())?;
    let data_root = data_dir(app.app_handle()).map_err(|_| "无法获取应用数据目录".to_owned())?;
    let recovery = services::cloud_sync::recover(&config_root);
    let app_state = match recovery.and_then(|_| storage::load_app_data(app.app_handle())) {
        Ok(data) => AppState::new(data),
        Err(err) => AppState::with_load_error(AppData::default(), Some(err)),
    };
    services::agent_cli::config_support::native_history::initialize(&data_root, &config_root)?;
    services::agent_cli::initialize_discovery_cache(data_root.join("agent-discovery-cache"));
    services::agent_cli::lifecycle::initialize_journal(data_root.join("agent-upgrades"));
    // updater/手动可见重启会继承原进程参数；一次性环境标记用于覆盖继承的
    // --silent-start，读取后立即删除，避免污染后续普通启动。
    let force_visible_start = services::app_updater::consume_visible_relaunch();
    let startup_settings = app_state
        .data
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .settings
        .clone();
    let silent_start = should_start_silently(
        force_visible_start,
        std::env::args().any(|arg| arg == "--silent-start"),
        startup_settings.launch_at_login_minimized,
    );
    app.manage(app_state);
    // 日志要先于其余后台服务安装：它们的告警都往这里写。
    if let Ok(directory) = log_dir(app.app_handle()) {
        services::logging::init(&directory, &startup_settings);
        // 每次启动留一行锚点：用户发来的日志能对上版本和目录。
        log::info!(
            "BalanceHub {} 启动（{}），日志目录 {}",
            app.package_info().version,
            if cfg!(debug_assertions) {
                "开发版"
            } else {
                "安装版"
            },
            directory.display()
        );
    }
    restore_window_state(app, &startup_settings);
    if let Ok(sync) = app.state::<AppState>().cloud_sync(app.app_handle()) {
        sync.start_worker(app.app_handle().clone());
    }

    // Agent runtime persistence is optional. Initialization failures remain
    // available through IPC and must not prevent the main App from starting.
    let agent_runtime =
        services::agent_runtime::service::AgentRuntimeService::from_app(app.app_handle());
    app.manage(agent_runtime.clone());
    agent_runtime.start(app.app_handle());

    // 单实例插件已在 setup 前完成仲裁，只有主实例会启动后台调度。
    services::scheduler::start(app.app_handle());
    // Agent 自动更新策略的执行者：核对升级记录、按设置扫描新版本并逐个安装。
    services::agent_cli::lifecycle::start_worker(app.app_handle());

    // 清扫历史残留的临时 CLI 目录/测活隔离 HOME（可能含明文凭据）。
    std::thread::spawn(services::temporary_cli::cleanup_stale);

    install_close_behavior(app);
    let tray_available = build_tray_with_linux_fallback(app)?;
    tray::set_available(app.app_handle(), tray_available);
    if tray_available {
        tray::refresh_from_state(app.app_handle());
    }

    if silent_start && tray_available {
        #[cfg(target_os = "macos")]
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    } else {
        // Linux 无托盘时不能静默隐藏，否则用户没有恢复入口。
        tray::show_main_window(app.app_handle());
    }
    Ok(())
}

fn restore_window_state(app: &App, settings: &crate::models::AppSettings) {
    if !settings.window_restore_state {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    use tauri_plugin_window_state::WindowExt;
    if let Err(err) = window.restore_state(window_state_flags()) {
        log::warn!("恢复上次窗口位置失败：{err}");
    }
}

fn install_close_behavior(app: &App) {
    if let Some(window) = app.get_webview_window("main") {
        let app_handle = window.app_handle().clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 后台刷新、签到和测活都依赖进程存活；只有用户明确选择「退出」
                // 或者根本没有托盘可回退时才真的结束进程。
                let stays_in_background = tray::is_available(&app_handle)
                    && current_close_action(&app_handle) == WindowCloseAction::HideToTray;
                if !stays_in_background {
                    app_handle.exit(0);
                    return;
                }
                api.prevent_close();
                tray::hide_main_window(&app_handle);
                hint_background_running(&app_handle);
            }
        });
    }
}

/// 第一次关窗时说明进程还在跑；之后只留托盘入口，不再打扰。
fn hint_background_running(app: &AppHandle) {
    let first_time = app.try_state::<AppState>().is_some_and(|state| {
        !state
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .settings
            .window_close_hint_shown
    });
    if !first_time {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("BalanceHub 仍在后台运行")
        .body("刷新、签到和测活会继续；点菜单栏图标重新打开，托盘菜单里可以退出。")
        .show();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = ProviderService::new(&handle).mark_close_hint_shown().await {
            log::warn!("记录关窗提示状态失败：{error}");
        }
    });
}

/// 关闭行为每次关窗时现读设置，设置页改完立即生效，不需要重装监听。
fn current_close_action(app: &AppHandle) -> WindowCloseAction {
    app.try_state::<AppState>()
        .map(|state| {
            state
                .data
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .settings
                .window_close_action
        })
        .unwrap_or(WindowCloseAction::HideToTray)
}

fn current_tray_click_action(app: &AppHandle) -> TrayClickAction {
    app.try_state::<AppState>()
        .map(|state| {
            state
                .data
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .settings
                .window_tray_click_action
        })
        .unwrap_or_default()
}

fn build_tray_with_linux_fallback(app: &App) -> Result<bool, Box<dyn std::error::Error>> {
    match build_tray(app) {
        Ok(()) => Ok(true),
        Err(err) => {
            #[cfg(target_os = "linux")]
            {
                log::warn!("BalanceHub 托盘初始化失败，回退为普通窗口模式: {err}");
                Ok(false)
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(err.into())
            }
        }
    }
}

fn build_tray(app: &App) -> tauri::Result<()> {
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
        .on_tray_icon_event(|tray_icon, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray_icon.app_handle();
                match current_tray_click_action(app) {
                    TrayClickAction::ShowWindow => tray::show_main_window(app),
                    TrayClickAction::ToggleWindow => tray::toggle_main_window(app),
                }
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
    Ok(())
}

/// 开发版的隔离根目录。钉在临时目录是因为设置页没有入口，用环境变量
/// BALANCEHUB_DEV_ROOT 覆盖即可清空；配置与数据分成两个子目录，避免原生
/// 配置扫描把备份或日志当成用户的真实配置文件。
fn development_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os(BALANCEHUB_DEV_ROOT_ENV) {
        let root = PathBuf::from(value);
        return Ok(root.canonicalize().unwrap_or(root));
    }
    app.path()
        .temp_dir()
        .map_err(|_| "无法获取临时目录".to_string())
        .map(|dir| dir.join(format!("balancehub-dev-{}", app.config().identifier)))
}

const BALANCEHUB_DEV_ROOT_ENV: &str = "BALANCEHUB_DEV_ROOT";
const DEV_CONFIG_DIR: &str = "config";
const DEV_DATA_DIR: &str = "data";
const DEV_LOG_DIR: &str = "logs";

/// 应用配置目录。开发构建在前台设置页里也不存在开发版入口，直接用临时根目录，
/// 让配置写入、备份、WebDAV 同步全部落在可随手删除的位置。
pub(crate) fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return development_root(app).map(|root| root.join(DEV_CONFIG_DIR));
    }
    app.path()
        .app_config_dir()
        .map_err(|error| error.to_string())
}

/// 应用数据目录。临时 CLI、测活隔离 HOME、Agent 运行目录都以它为基准。
pub(crate) fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return development_root(app).map(|root| root.join(DEV_DATA_DIR));
    }
    app.path().app_data_dir().map_err(|error| error.to_string())
}

/// 日志目录。开发版同样落在隔离根目录下，不和安装版的日志文件混在一起。
pub(crate) fn log_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return development_root(app).map(|root| root.join(DEV_LOG_DIR));
    }
    app.path().app_log_dir().map_err(|error| error.to_string())
}

/// 开发版专用：把演示卡片追加到隔离的开发版配置里，真实站点与设置原样保留。
/// 发布构建里这个 command 只返回错误，前端只在开发版显示入口，因此安装版
/// 永远拿不到演示数据。
#[tauri::command]
async fn install_dev_sample_data(app: AppHandle) -> Result<usize, String> {
    #[cfg(debug_assertions)]
    {
        crate::commands::run_blocking("写入演示数据", move || {
            ProviderService::new(&app).merge_dev_sample()
        })
        .await
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = app;
        Err("发布构建不提供演示数据".to_string())
    }
}

fn should_start_silently(
    force_visible_start: bool,
    has_silent_start_arg: bool,
    launch_at_login_minimized: bool,
) -> bool {
    !force_visible_start && has_silent_start_arg && launch_at_login_minimized
}

#[cfg(test)]
mod tests {
    use super::should_start_silently;

    #[test]
    fn visible_relaunch_overrides_inherited_silent_start() {
        assert!(!should_start_silently(true, true, true));
        assert!(should_start_silently(false, true, true));
        assert!(!should_start_silently(false, false, true));
        assert!(!should_start_silently(false, true, false));
    }
}
