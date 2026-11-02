use crate::{
    models::{LoginAccount, LoginPlatform},
    services::{
        provider_browser_login::{self, ProviderBrowserLoginTask},
        provider_service::{
            CredentialKind, LoginAccountSummary, LoginCookieSummary, ProviderCredentialDetails,
            ProviderService,
        },
    },
};
use tauri::AppHandle;

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
pub(crate) async fn get_provider_credentials(
    _app: AppHandle,
    id: String,
) -> Result<ProviderCredentialDetails, String> {
    super::run_blocking("读取站点凭据", move || {
        ProviderService::new().credential_details(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_provider_credential(
    _app: AppHandle,
    id: String,
    kind: CredentialKind,
    revision: u64,
) -> Result<String, String> {
    super::run_blocking("读取站点凭据内容", move || {
        ProviderService::new().read_credential(&id, kind, revision)
    })
    .await
}

#[tauri::command]
pub(crate) async fn clear_provider_credential(
    _app: AppHandle,
    id: String,
    kind: CredentialKind,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .clear_credential(id, kind, revision)
        .await
}

#[tauri::command]
pub(crate) async fn validate_provider_credentials(
    _app: AppHandle,
    id: String,
) -> Result<ProviderCredentialDetails, String> {
    ProviderService::new().validate_credentials(id).await
}
