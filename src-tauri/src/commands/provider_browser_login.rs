use crate::{
    models::ProviderInput,
    services::provider_browser_login::{self, ProviderBrowserLoginTask},
};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn start_provider_browser_login(
    app: AppHandle,
    input: ProviderInput,
    login_account_id: String,
) -> Result<ProviderBrowserLoginTask, String> {
    super::run_blocking("创建登录任务", move || {
        provider_browser_login::start(&app, input, login_account_id)
    })
    .await
}

#[tauri::command]
pub(crate) fn list_provider_browser_logins() -> Result<Vec<ProviderBrowserLoginTask>, String> {
    provider_browser_login::list()
}

#[tauri::command]
pub(crate) fn cancel_provider_browser_login(run_id: String) -> Result<(), String> {
    provider_browser_login::cancel(&run_id)
}

#[tauri::command]
pub(crate) async fn show_provider_login_window(run_id: String) -> Result<(), String> {
    provider_browser_login::show_window(&run_id).await
}
