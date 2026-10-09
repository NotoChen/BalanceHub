use crate::{
    commands::{
        agent_assets::*, agent_catalog::*, agent_configuration::*, agent_lifecycle::*,
        agent_overview::*, agent_session_resume::*, agent_sessions::*, app::*, browser_runtime::*,
        cli::*, login_accounts::*, provider::*, provider_browser_login::*,
    },
    models::AppData,
    services::{self, app_updater::AppUpdaterState},
    state::AppState,
    storage, tray,
};
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, Manager, WindowEvent,
};

pub(crate) fn run() {
    tauri::Builder::default()
        // 必须是第一个插件：在 setup 和后台调度启动前拦截第二个实例。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .manage(AppUpdaterState::default())
        .manage(tray::TrayAvailability::default())
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
            host_platform,
            open_ccswitch_deeplink,
            open_project_repository,
            launch_temporary_cli,
            preview_temporary_cli_launch,
            get_agent_session_scope,
            query_agent_sessions,
            get_agent_session_detail,
            cancel_agent_session_query,
            resume_agent_session,
            get_agent_session_resume_operation,
            list_agent_session_resume_operations,
            cancel_agent_session_resume_operation,
            get_cli_session_index_status,
            clear_cli_session_index,
            get_cli_runtime_snapshot,
            get_agent_runtime_snapshot,
            get_agent_runtime_status,
            activate_agent_runtime,
            get_temporary_cli_instance,
            forget_workspace,
            browse_workspace_directories,
            preview_cli_config,
            list_agent_configuration_sources,
            read_agent_configuration_source,
            begin_agent_configuration_edit,
            begin_agent_resource_edit,
            open_agent_resource_link,
            plan_agent_configuration_save,
            apply_agent_configuration_plan,
            discard_agent_configuration_edit,
            get_agent_configuration_operation,
            list_agent_configuration_operations,
            cancel_agent_configuration_operation,
            open_agent_configuration_source,
            load_app_data,
            save_provider,
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
            get_provider_request_logs,
            change_provider_password,
            get_provider_check_in_records,
            probe_provider_capabilities,
            sync_available_models,
            get_provider_invite_link,
            get_site_announcements,
            mark_site_announcement_read,
            refresh_all_providers_with_progress,
            refresh_providers,
            check_in_provider,
            open_provider_site,
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
    let app_state = match storage::load_app_data(app.app_handle()) {
        Ok(data) => AppState::new(data),
        Err(err) => AppState::with_load_error(AppData::default(), Some(err)),
    };
    if let Ok(root) = app.path().app_data_dir() {
        services::agent_cli::initialize_discovery_cache(root.join("agent-discovery-cache"));
        services::agent_cli::lifecycle::initialize_journal(root.join("agent-upgrades"));
    }
    // updater/手动可见重启会继承原进程参数；一次性环境标记用于覆盖继承的
    // --silent-start，读取后立即删除，避免污染后续普通启动。
    let force_visible_start = services::app_updater::consume_visible_relaunch();
    let silent_start = should_start_silently(
        force_visible_start,
        std::env::args().any(|arg| arg == "--silent-start"),
        app_state
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .settings
            .launch_at_login_minimized,
    );
    app.manage(app_state);

    // Agent runtime persistence is optional. Initialization failures remain
    // available through IPC and must not prevent the main App from starting.
    let agent_runtime =
        services::agent_runtime::service::AgentRuntimeService::from_app(app.app_handle());
    app.manage(agent_runtime.clone());
    agent_runtime.start(app.app_handle());

    // 单实例插件已在 setup 前完成仲裁，只有主实例会启动后台调度。
    services::scheduler::start(app.app_handle());

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

fn install_close_behavior(app: &App) {
    if let Some(window) = app.get_webview_window("main") {
        let app_handle = window.app_handle().clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if tray::is_available(&app_handle) {
                    api.prevent_close();
                    tray::hide_main_window(&app_handle);
                } else {
                    app_handle.exit(0);
                }
            }
        });
    }
}

fn build_tray_with_linux_fallback(app: &App) -> Result<bool, Box<dyn std::error::Error>> {
    match build_tray(app) {
        Ok(()) => Ok(true),
        Err(err) => {
            #[cfg(target_os = "linux")]
            {
                eprintln!("BalanceHub 托盘初始化失败，回退为普通窗口模式: {err}");
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
                tray::show_main_window(tray_icon.app_handle());
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
