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
