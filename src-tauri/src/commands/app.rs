use crate::{
    contracts::AppDataView,
    models::{AppDataTransferResult, AppSettings, Provider},
    platform::cc_switch,
    services::{
        app_updater::{self, AppUpdateDownloadEvent, AppUpdateInfo},
        liveness::preview_prompts,
        notifications::{self, NotificationSendResult},
        provider_service::ProviderService,
    },
    tray,
};
use tauri::{ipc::Channel, AppHandle};

#[tauri::command]
pub(crate) fn host_platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
pub(crate) fn open_ccswitch_deeplink(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = validate_ccswitch_deeplink(&url)?;
    cc_switch::open(&app, trimmed)
}

fn validate_ccswitch_deeplink(url: &str) -> Result<&str, String> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "无效的 CC Switch 深链".to_string())?;
    if parsed.scheme() != "ccswitch"
        || parsed.host_str() != Some("v1")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.path() != "/import"
        || parsed.query().is_none()
        || parsed.fragment().is_some()
    {
        return Err("无效的 CC Switch 深链".to_string());
    }
    Ok(trimmed)
}

#[tauri::command]
pub(crate) fn load_app_data(app: AppHandle) -> Result<AppDataView, String> {
    let data = ProviderService::new(&app).load_app_data()?;
    tray::update_tooltip(&app, &data.providers);
    Ok(data.into())
}

#[tauri::command]
pub(crate) fn save_settings(app: AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    let settings = ProviderService::new(&app).save_settings(settings)?;
    tray::refresh_from_state(&app);
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn send_app_notification(
    app: AppHandle,
    settings: AppSettings,
    provider: Option<Provider>,
    title: String,
    markdown: String,
    ignore_switch: bool,
) -> Result<NotificationSendResult, String> {
    if let Some(provider) = provider {
        Ok(notifications::send_provider_notification(
            &app,
            &settings,
            &provider,
            title,
            markdown,
            ignore_switch,
        )
        .await)
    } else {
        Ok(notifications::send_configured_notification(
            &app,
            &settings,
            title,
            markdown,
            ignore_switch,
        )
        .await)
    }
}

#[tauri::command]
pub(crate) fn export_app_data(
    app: AppHandle,
    path: String,
) -> Result<AppDataTransferResult, String> {
    ProviderService::new(&app).export_app_data(path)
}

#[tauri::command]
pub(crate) fn import_app_data(
    app: AppHandle,
    path: String,
) -> Result<AppDataTransferResult, String> {
    let (_data, result) = ProviderService::new(&app).import_app_data(path)?;
    tray::refresh_from_state(&app);
    Ok(result)
}

#[tauri::command]
pub(crate) fn preview_liveness_prompts(settings: AppSettings, count: usize) -> Vec<String> {
    preview_prompts(&settings, count)
}

#[tauri::command]
pub(crate) async fn check_app_update(app: AppHandle) -> Result<Option<AppUpdateInfo>, String> {
    app_updater::check(&app).await
}

#[tauri::command]
pub(crate) async fn install_app_update(
    app: AppHandle,
    on_event: Channel<AppUpdateDownloadEvent>,
) -> Result<(), String> {
    app_updater::install(&app, on_event).await
}

#[tauri::command]
pub(crate) fn cancel_app_update(app: AppHandle) -> Result<(), String> {
    app_updater::cancel_download(&app)
}

#[tauri::command]
pub(crate) fn clear_pending_app_update(app: AppHandle) -> Result<(), String> {
    app_updater::clear_pending(&app)
}

#[tauri::command]
pub(crate) fn cancel_visible_relaunch() {
    app_updater::cancel_visible_relaunch();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_only_ccswitch_import_links() {
        assert_eq!(
            validate_ccswitch_deeplink(" ccswitch://v1/import?target=codex "),
            Ok("ccswitch://v1/import?target=codex")
        );
        for invalid in [
            "https://v1/import?target=codex",
            "ccswitch://v2/import?target=codex",
            "ccswitch://v1/export?target=codex",
            "ccswitch://v1/import",
            "ccswitch://user@v1/import?target=codex",
            "ccswitch://v1:1234/import?target=codex",
            "ccswitch://v1/import?target=codex#fragment",
        ] {
            assert_eq!(
                validate_ccswitch_deeplink(invalid),
                Err("无效的 CC Switch 深链".to_string())
            );
        }
    }
}
