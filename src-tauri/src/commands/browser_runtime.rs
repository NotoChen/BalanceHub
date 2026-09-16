use super::run_blocking;
use crate::services::browser_runtime::{self, BrowserRuntimeStatus};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn get_browser_runtime_status(
    app: AppHandle,
    force: bool,
) -> Result<BrowserRuntimeStatus, String> {
    run_blocking("检测浏览器签到组件", move || {
        browser_runtime::status(&app, force)
    })
    .await
}

#[tauri::command]
pub(crate) fn install_browser_runtime(
    app: AppHandle,
    include_browser: bool,
) -> Result<BrowserRuntimeStatus, String> {
    browser_runtime::start_install(&app, include_browser)
}

#[tauri::command]
pub(crate) fn cancel_browser_runtime_install() -> Result<(), String> {
    browser_runtime::cancel_install()
}

#[tauri::command]
pub(crate) async fn uninstall_browser_runtime(
    app: AppHandle,
) -> Result<BrowserRuntimeStatus, String> {
    run_blocking("卸载浏览器签到组件", move || {
        browser_runtime::uninstall(&app)
    })
    .await
}
