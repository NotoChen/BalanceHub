use crate::{
    contracts::AppDataImportResultView,
    models::{AppDataTransferResult, AppSettings, Provider},
    services::{
        app_updater::{self, AppUpdateDownloadEvent, AppUpdateInfo},
        logging,
        provider_service::ProviderService,
    },
    tray,
};
use serde_json::json;
use tauri::{ipc::Channel, AppHandle};
use tauri_plugin_opener::OpenerExt;

use super::{dispatch, dispatch_value, run_blocking};

#[tauri::command]
pub(crate) fn host_platform() -> &'static str {
    std::env::consts::OS
}

/// 前端按后台任务中心的活动任务同步退出守卫；桌面专属，服务端没有窗口退出。
#[tauri::command]
pub(crate) fn set_app_running_tasks(
    state: tauri::State<'_, crate::state::ExitGuardState>,
    active: bool,
) {
    state.set_running(active);
}

/// 用户在退出确认里确认后执行；由用户主动触发，不再回到关闭守卫。
#[tauri::command]
pub(crate) fn quit_app(app: AppHandle) {
    app.exit(0);
}

const PROJECT_REPOSITORY_URL: &str = "https://github.com/NotoChen/BalanceHub";

#[tauri::command]
pub(crate) async fn open_project_repository(app: AppHandle) -> Result<(), String> {
    run_blocking("打开 BalanceHub GitHub", move || {
        app.opener()
            .open_url(PROJECT_REPOSITORY_URL, None::<&str>)
            .map_err(|err| format!("无法打开 GitHub: {err}"))
    })
    .await
}

/// 所有来自非本应用内容的链接（Agent 资源文档、站点公告正文）都走这里：
/// 只放行不带凭据的 HTTP/HTTPS 地址，交给系统默认浏览器而不是 WebView。
#[tauri::command]
pub(crate) async fn open_external_link(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| "链接无效".to_owned())?;
    if !matches!(parsed.scheme(), "https" | "http")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("仅支持不含账号凭据的 HTTP 或 HTTPS 链接".to_owned());
    }
    run_blocking("打开外部链接", move || {
        app.opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|_| "无法打开链接".to_owned())
    })
    .await
}

#[tauri::command]
pub(crate) async fn load_app_data(app: AppHandle) -> Result<serde_json::Value, String> {
    let data = dispatch_value("load_app_data", json!({})).await?;
    tray::refresh_from_state(&app);
    Ok(data)
}

#[tauri::command]
pub(crate) async fn save_settings(
    settings: AppSettings,
    expected: AppSettings,
) -> Result<AppSettings, String> {
    let settings: AppSettings = dispatch(
        "save_settings",
        json!({ "settings": settings, "expected": expected }),
    )
    .await?;
    // 级别与保留期改完立即生效，不用重启托盘进程。
    logging::apply_settings(&settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn open_log_directory(app: AppHandle) -> Result<(), String> {
    let directory = crate::desktop::log_dir(&app)?;
    run_blocking("打开日志目录", move || {
        std::fs::create_dir_all(&directory).map_err(|err| format!("无法创建日志目录: {err}"))?;
        app.opener()
            .open_path(directory.to_string_lossy(), None::<&str>)
            .map_err(|err| format!("无法打开日志目录: {err}"))
    })
    .await
}

#[tauri::command]
pub(crate) async fn open_data_directory(app: AppHandle) -> Result<(), String> {
    let directory = crate::desktop::data_dir(&app)?;
    run_blocking("打开数据目录", move || {
        std::fs::create_dir_all(&directory).map_err(|err| format!("无法创建数据目录: {err}"))?;
        app.opener()
            .open_path(directory.to_string_lossy(), None::<&str>)
            .map_err(|err| format!("无法打开数据目录: {err}"))
    })
    .await
}

/// 配置目录是 `config.json` / `state.json` 的落点；外部编辑热重载完全依赖
/// 用户能直接找到并改这两个文件，所以入口和数据、日志目录放在一起。
#[tauri::command]
pub(crate) async fn open_config_directory(app: AppHandle) -> Result<(), String> {
    let directory = crate::desktop::config_dir(&app)?;
    run_blocking("打开配置目录", move || {
        std::fs::create_dir_all(&directory).map_err(|err| format!("无法创建配置目录: {err}"))?;
        app.opener()
            .open_path(directory.to_string_lossy(), None::<&str>)
            .map_err(|err| format!("无法打开配置目录: {err}"))
    })
    .await
}

#[tauri::command]
pub(crate) async fn send_app_notification(
    settings: AppSettings,
    provider: Option<Provider>,
    title: String,
    markdown: String,
    ignore_switch: bool,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "send_app_notification",
        json!({
            "settings": settings,
            "provider": provider,
            "title": title,
            "markdown": markdown,
            "ignoreSwitch": ignore_switch,
        }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn export_app_data(
    _app: AppHandle,
    path: String,
) -> Result<AppDataTransferResult, String> {
    run_blocking("导出应用配置", move || {
        ProviderService::new().export_app_data(path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn import_app_data(
    app: AppHandle,
    path: String,
) -> Result<AppDataImportResultView, String> {
    let _task_app = app.clone();
    let (data, transfer) = run_blocking("导入应用配置", move || {
        ProviderService::new().import_app_data(path)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(AppDataImportResultView {
        data: data.into(),
        transfer,
    })
}

/// 启动读取失败且 config.json（或旧版 data.json）损坏时，用同目录最近的自动备份原地恢复。
#[tauri::command]
pub(crate) async fn restore_latest_backup() -> Result<serde_json::Value, String> {
    dispatch_value("restore_latest_backup", json!({})).await
}

#[tauri::command]
pub(crate) fn preview_liveness_prompts(settings: AppSettings, count: usize) -> Vec<String> {
    balancehub_core::services::liveness::preview_prompts(&settings, count)
}

#[tauri::command]
pub(crate) async fn check_app_update(app: AppHandle) -> Result<Option<AppUpdateInfo>, String> {
    app_updater::check(&app).await
}

#[tauri::command]
pub(crate) async fn install_app_update(
    app: AppHandle,
    on_event: Channel<AppUpdateDownloadEvent>,
) -> Result<(), String> {
    app_updater::install(&app, on_event).await
}

#[tauri::command]
pub(crate) fn cancel_app_update(app: AppHandle) -> Result<(), String> {
    app_updater::cancel_download(&app)
}

#[tauri::command]
pub(crate) fn clear_pending_app_update(app: AppHandle) -> Result<(), String> {
    app_updater::clear_pending(&app)
}

#[tauri::command]
pub(crate) fn cancel_visible_relaunch() {
    app_updater::cancel_visible_relaunch();
}
