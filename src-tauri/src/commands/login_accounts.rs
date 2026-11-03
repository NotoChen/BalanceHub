use crate::{
    models::{LoginAccount, LoginPlatform},
    services::{
        provider_browser_login::{self, ProviderBrowserLoginTask},
        provider_service::{LoginAccountSummary, LoginCookieSummary, ProviderService},
    },
};
use serde_json::json;
use tauri::AppHandle;

use super::dispatch_value;

#[tauri::command]
pub(crate) async fn list_login_accounts(
    _app: AppHandle,
) -> Result<Vec<LoginAccountSummary>, String> {
    super::run_blocking("读取登录账号", move || {
        ProviderService::new().list_login_accounts()
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_login_account(
    _app: AppHandle,
    name: String,
    platform: LoginPlatform,
) -> Result<LoginAccount, String> {
    super::run_blocking("新增登录账号", move || {
        ProviderService::new().create_login_account(name, platform)
    })
    .await
}

#[tauri::command]
pub(crate) async fn update_login_account(
    _app: AppHandle,
    id: String,
    name: String,
    platform: LoginPlatform,
) -> Result<(), String> {
    super::run_blocking("更新登录账号", move || {
        ProviderService::new().update_login_account(id, name, platform)
    })
    .await
}

#[tauri::command]
pub(crate) async fn remove_login_account(
    _app: AppHandle,
    id: String,
    remove_entry: bool,
) -> Result<(), String> {
    super::run_blocking("清除本地登录账号", move || {
        ProviderService::new().remove_login_account(id, remove_entry)
    })
    .await
}

#[tauri::command]
pub(crate) async fn open_login_account(
    _app: AppHandle,
    id: String,
    authorizations: bool,
) -> Result<ProviderBrowserLoginTask, String> {
    super::run_blocking("打开登录账号", move || {
        provider_browser_login::start_account(id, authorizations)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_login_account_cookies(
    _app: AppHandle,
    id: String,
) -> Result<Vec<LoginCookieSummary>, String> {
    super::run_blocking("读取 Cookie 列表", move || {
        ProviderService::new().login_account_cookies(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_login_account_cookie(
    _app: AppHandle,
    id: String,
    cookie_id: String,
) -> Result<String, String> {
    super::run_blocking("读取 Cookie", move || {
        ProviderService::new().read_login_cookie(&id, &cookie_id)
    })
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
