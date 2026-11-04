use crate::models::ProviderInput;
use serde_json::json;

use super::dispatch_value;

#[tauri::command]
pub(crate) async fn start_provider_browser_login(
    input: ProviderInput,
    login_account_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "start_provider_browser_login",
        json!({ "input": input, "loginAccountId": login_account_id }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn list_provider_browser_logins() -> Result<serde_json::Value, String> {
    dispatch_value("list_provider_browser_logins", json!({})).await
}

#[tauri::command]
pub(crate) async fn cancel_provider_browser_login(
    run_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value("cancel_provider_browser_login", json!({ "runId": run_id })).await
}

#[tauri::command]
pub(crate) async fn show_provider_login_window(
    run_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value("show_provider_login_window", json!({ "runId": run_id })).await
}
