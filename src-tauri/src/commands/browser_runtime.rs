//! 浏览器组件命令。实现来自 `balancehub_core::api`，与服务端共用。

use serde_json::json;

use super::dispatch_value;
use crate::services::browser_runtime::BrowserSelection;

#[tauri::command]
pub(crate) async fn get_browser_runtime_status(force: bool) -> Result<serde_json::Value, String> {
    dispatch_value("get_browser_runtime_status", json!({ "force": force })).await
}

#[tauri::command]
pub(crate) async fn install_browser_runtime(
    selection: BrowserSelection,
    repair: bool,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "install_browser_runtime",
        json!({ "selection": selection, "repair": repair }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn select_browser_runtime_browser(
    selection: BrowserSelection,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "select_browser_runtime_browser",
        json!({ "selection": selection }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn inspect_browser_runtime_browser(
    path: String,
) -> Result<serde_json::Value, String> {
    dispatch_value("inspect_browser_runtime_browser", json!({ "path": path })).await
}

#[tauri::command]
pub(crate) async fn cancel_browser_runtime_install() -> Result<serde_json::Value, String> {
    dispatch_value("cancel_browser_runtime_install", json!({})).await
}

#[tauri::command]
pub(crate) async fn uninstall_browser_runtime() -> Result<serde_json::Value, String> {
    dispatch_value("uninstall_browser_runtime", json!({})).await
}
