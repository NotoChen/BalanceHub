use crate::{
    models::*,
    services::{provider_service::ProviderService, temporary_cli},
    state::AppState,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) async fn get_agent_model_catalog(
    app: AppHandle,
    context: AgentModelCatalogContext,
    refresh: bool,
) -> Result<AgentModelCatalog, String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    super::run_blocking("读取 Agent 模型目录", move || {
        crate::services::agent_cli::settings::read_model_catalog(&context, &settings, refresh)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_temporary_cli_settings(
    request: TemporaryCliSettingsRequest,
) -> Result<TemporaryCliSettingsCapabilities, String> {
    super::run_blocking("读取本次启动能力", move || {
        temporary_cli::run_capabilities(&request)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_temporary_cli_assets(
    app: AppHandle,
    request: TemporaryCliSettingsRequest,
    refresh: bool,
) -> Result<TemporaryCliAssetListing, String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .settings
        .clone();
    super::run_blocking("读取本次启动资产", move || {
        temporary_cli::run_asset_options(&settings, &request, refresh)
    })
    .await
}

#[tauri::command]
pub(crate) fn list_temporary_cli_profiles(app: AppHandle) -> Vec<TemporaryCliProfile> {
    app.state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .temporary_cli_profiles
        .clone()
}

#[tauri::command]
pub(crate) async fn save_temporary_cli_profile(
    app: AppHandle,
    profile: TemporaryCliProfile,
) -> Result<Vec<TemporaryCliProfile>, String> {
    super::run_blocking("保存启动方案", move || {
        ProviderService::new(&app).save_temporary_cli_profile(profile)
    })
    .await
}

#[tauri::command]
pub(crate) async fn delete_temporary_cli_profile(
    app: AppHandle,
    id: String,
) -> Result<Vec<TemporaryCliProfile>, String> {
    super::run_blocking("删除启动方案", move || {
        ProviderService::new(&app).delete_temporary_cli_profile(&id)
    })
    .await
}
