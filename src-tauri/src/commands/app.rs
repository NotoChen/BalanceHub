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
use tauri_plugin_opener::OpenerExt;

use super::run_blocking;

#[tauri::command]
pub(crate) fn host_platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
pub(crate) async fn open_ccswitch_deeplink(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = validate_ccswitch_deeplink(&url)?;
    let trimmed = trimmed.to_string();
    run_blocking("打开 CC Switch", move || cc_switch::open(&app, &trimmed)).await
}

const PROJECT_REPOSITORY_URL: &str = "https://github.com/NotoChen/BalanceHub";

#[tauri::command]
pub(crate) async fn open_project_repository(app: AppHandle) -> Result<(), String> {
    run_blocking("打开 BalanceHub GitHub", move || {
        app.opener()
            .open_url(PROJECT_REPOSITORY_URL, None::<&str>)
            .map_err(|err| format!("无法打开 GitHub: {err}"))
    })
    .await
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
pub(crate) async fn load_app_data(app: AppHandle) -> Result<AppDataView, String> {
    let data = {
        let task_app = app.clone();
        run_blocking("加载应用配置", move || {
            ProviderService::new(&task_app).load_app_data()
        })
        .await?
    };
    tray::update_tooltip(&app, &data.providers);
    Ok(data.into())
}

#[tauri::command]
pub(crate) async fn save_settings(
    app: AppHandle,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let task_app = app.clone();
    let settings = run_blocking("保存应用设置", move || {
        ProviderService::new(&task_app).save_settings(settings)
    })
    .await?;
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
pub(crate) async fn export_app_data(
    app: AppHandle,
    path: String,
) -> Result<AppDataTransferResult, String> {
    run_blocking("导出应用配置", move || {
        ProviderService::new(&app).export_app_data(path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn import_app_data(
    app: AppHandle,
    path: String,
) -> Result<AppDataTransferResult, String> {
    let task_app = app.clone();
    let (_data, result) = run_blocking("导入应用配置", move || {
        ProviderService::new(&task_app).import_app_data(path)
    })
    .await?;
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
