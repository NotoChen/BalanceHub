use super::run_blocking;
use crate::services::browser_runtime::{self, BrowserInfo, BrowserRuntimeStatus, BrowserSelection};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn get_browser_runtime_status(
    app: AppHandle,
    force: bool,
) -> Result<BrowserRuntimeStatus, String> {
    run_blocking("检测浏览器组件", move || {
        browser_runtime::status(&app, force)
    })
    .await
}

#[tauri::command]
pub(crate) fn install_browser_runtime(
    app: AppHandle,
    selection: BrowserSelection,
    repair: bool,
) -> Result<BrowserRuntimeStatus, String> {
    browser_runtime::start_install(&app, selection, repair)
}

#[tauri::command]
pub(crate) fn select_browser_runtime_browser(
    app: AppHandle,
    selection: BrowserSelection,
) -> Result<BrowserRuntimeStatus, String> {
    browser_runtime::select_browser(&app, selection)
}

#[tauri::command]
pub(crate) async fn inspect_browser_runtime_browser(path: String) -> Result<BrowserInfo, String> {
    run_blocking("读取浏览器程序", move || {
        browser_runtime::inspect_browser(std::path::Path::new(&path))
    })
    .await
}

#[tauri::command]
pub(crate) fn cancel_browser_runtime_install() -> Result<(), String> {
    browser_runtime::cancel_install()
}

#[tauri::command]
pub(crate) async fn uninstall_browser_runtime(
    app: AppHandle,
) -> Result<BrowserRuntimeStatus, String> {
    run_blocking("卸载浏览器组件", move || {
        browser_runtime::uninstall(&app)
    })
    .await
}
