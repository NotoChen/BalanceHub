use crate::models::LoginPlatform;
use serde_json::json;

use super::dispatch_value;

#[tauri::command]
pub(crate) async fn list_login_accounts() -> Result<serde_json::Value, String> {
    dispatch_value("list_login_accounts", json!({})).await
}

#[tauri::command]
pub(crate) async fn create_login_account(
    name: String,
    platform: LoginPlatform,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "create_login_account",
        json!({ "name": name, "platform": platform }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn update_login_account(
    id: String,
    name: String,
    platform: LoginPlatform,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "update_login_account",
        json!({ "id": id, "name": name, "platform": platform }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn remove_login_account(
    id: String,
    remove_entry: bool,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "remove_login_account",
        json!({ "id": id, "removeEntry": remove_entry }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn open_login_account(
    id: String,
    authorizations: bool,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "open_login_account",
        json!({ "id": id, "authorizations": authorizations }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn list_login_account_cookies(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("list_login_account_cookies", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn read_login_account_cookie(
    id: String,
    cookie_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "read_login_account_cookie",
        json!({ "id": id, "cookieId": cookie_id }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_provider_credentials(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("get_provider_credentials", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn read_provider_credential(
    id: String,
    kind: crate::services::provider_service::CredentialKind,
    revision: u64,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "read_provider_credential",
        json!({ "id": id, "kind": kind, "revision": revision }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn clear_provider_credential(
    id: String,
    kind: crate::services::provider_service::CredentialKind,
    revision: u64,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "clear_provider_credential",
        json!({ "id": id, "kind": kind, "revision": revision }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn validate_provider_credentials(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("validate_provider_credentials", json!({ "id": id })).await
}
