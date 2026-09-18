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
    app: AppHandle,
) -> Result<Vec<LoginAccountSummary>, String> {
    super::run_blocking("读取登录账号", move || {
        ProviderService::new(&app).list_login_accounts()
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_login_account(
    app: AppHandle,
    name: String,
    platform: LoginPlatform,
) -> Result<LoginAccount, String> {
    super::run_blocking("新增登录账号", move || {
        ProviderService::new(&app).create_login_account(name, platform)
    })
    .await
}

#[tauri::command]
pub(crate) async fn update_login_account(
    app: AppHandle,
    id: String,
    name: String,
    platform: LoginPlatform,
) -> Result<(), String> {
    super::run_blocking("更新登录账号", move || {
        ProviderService::new(&app).update_login_account(id, name, platform)
    })
    .await
}

#[tauri::command]
pub(crate) async fn remove_login_account(
    app: AppHandle,
    id: String,
    remove_entry: bool,
) -> Result<(), String> {
    super::run_blocking("清除本地登录账号", move || {
        ProviderService::new(&app).remove_login_account(id, remove_entry)
    })
    .await
}

#[tauri::command]
pub(crate) async fn open_login_account(
    app: AppHandle,
    id: String,
    authorizations: bool,
) -> Result<ProviderBrowserLoginTask, String> {
    super::run_blocking("打开登录账号", move || {
        provider_browser_login::start_account(&app, id, authorizations)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_login_account_cookies(
    app: AppHandle,
    id: String,
) -> Result<Vec<LoginCookieSummary>, String> {
    super::run_blocking("读取 Cookie 列表", move || {
        ProviderService::new(&app).login_account_cookies(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_login_account_cookie(
    app: AppHandle,
    id: String,
    cookie_id: String,
) -> Result<String, String> {
    super::run_blocking("读取 Cookie", move || {
        ProviderService::new(&app).read_login_cookie(&id, &cookie_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_provider_credentials(
    app: AppHandle,
    id: String,
) -> Result<ProviderCredentialDetails, String> {
    super::run_blocking("读取站点凭据", move || {
        ProviderService::new(&app).credential_details(&id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_provider_credential(
    app: AppHandle,
    id: String,
    kind: CredentialKind,
    revision: u64,
) -> Result<String, String> {
    super::run_blocking("读取站点凭据内容", move || {
        ProviderService::new(&app).read_credential(&id, kind, revision)
    })
    .await
}

#[tauri::command]
pub(crate) async fn clear_provider_credential(
    app: AppHandle,
    id: String,
    kind: CredentialKind,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new(&app)
        .clear_credential(id, kind, revision)
        .await
}

#[tauri::command]
pub(crate) async fn validate_provider_credentials(
    app: AppHandle,
    id: String,
) -> Result<ProviderCredentialDetails, String> {
    ProviderService::new(&app).validate_credentials(id).await
}
