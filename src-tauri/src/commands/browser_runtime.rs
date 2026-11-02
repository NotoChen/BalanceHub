use super::run_blocking;
use crate::services::browser_runtime::{self, BrowserInfo, BrowserRuntimeStatus, BrowserSelection};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn get_browser_runtime_status(
    _app: AppHandle,
    force: bool,
) -> Result<BrowserRuntimeStatus, String> {
    run_blocking("检测浏览器组件", move || {
        browser_runtime::status(force)
    })
    .await
}

#[tauri::command]
pub(crate) fn install_browser_runtime(
    _app: AppHandle,
    selection: BrowserSelection,
    repair: bool,
) -> Result<BrowserRuntimeStatus, String> {
    browser_runtime::start_install(selection, repair)
}

#[tauri::command]
pub(crate) fn select_browser_runtime_browser(
    _app: AppHandle,
    selection: BrowserSelection,
) -> Result<BrowserRuntimeStatus, String> {
    browser_runtime::select_browser(selection)
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
    _app: AppHandle,
) -> Result<BrowserRuntimeStatus, String> {
    run_blocking("卸载浏览器组件", move || {
        browser_runtime::uninstall()
    })
    .await
}
