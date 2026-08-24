use crate::{
    contracts::{
        ProviderCapabilityProbeResultView, ProviderModelSyncResultView, ProviderSaveResultView,
        ProviderView, RefreshResultView,
    },
    models::{
        ProviderApiKeyOption, ProviderBatchProgressEvent, ProviderCheckInRecordsResult,
        ProviderCheckInResult, ProviderConnectionTestResult, ProviderCredentialCompletionResult,
        ProviderInput, ProviderProtocolDetectionResult, ProviderRemovalResult,
        ProviderRequestLogsQuery, ProviderRequestLogsResult, ProviderSaveOptions,
        ProviderSiteProbeResult, ProviderUsageSummary, SiteAnnouncementsSnapshot,
    },
    services::provider_service::ProviderService,
    tray,
};
use tauri::{ipc::Channel, AppHandle};

use super::run_blocking;

#[tauri::command]
pub(crate) async fn save_provider(
    app: AppHandle,
    input: ProviderInput,
    options: Option<ProviderSaveOptions>,
) -> Result<ProviderSaveResultView, String> {
    let task_app = app.clone();
    let result = run_blocking("保存中转站", move || {
        ProviderService::new(&task_app).save_provider(input, options.unwrap_or_default())
    })
    .await?;
    if result.saved {
        tray::refresh_from_state(&app);
    }
    Ok(result.into())
}

#[tauri::command]
pub(crate) async fn remove_provider(
    app: AppHandle,
    id: String,
) -> Result<ProviderRemovalResult, String> {
    let task_app = app.clone();
    let result = run_blocking("删除中转站", move || {
        ProviderService::new(&task_app).remove_provider(id)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(result)
}

#[tauri::command]
pub(crate) async fn reorder_providers(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let task_app = app.clone();
    let order = run_blocking("调整中转站顺序", move || {
        ProviderService::new(&task_app).reorder_providers(ids)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(order)
}

#[tauri::command]
pub(crate) async fn complete_provider_credentials(
    app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderCredentialCompletionResult, String> {
    ProviderService::new(&app).complete_credentials(input).await
}

#[tauri::command]
pub(crate) async fn test_provider_connection(
    app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderConnectionTestResult, String> {
    ProviderService::new(&app).test_connection(input).await
}

#[tauri::command]
pub(crate) async fn probe_provider_site(
    app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderSiteProbeResult, String> {
    ProviderService::new(&app).probe_site(input).await
}

#[tauri::command]
pub(crate) async fn detect_provider_protocol(
    app: AppHandle,
    input: ProviderInput,
) -> ProviderProtocolDetectionResult {
    ProviderService::new(&app).detect_protocol(input).await
}

#[tauri::command]
pub(crate) async fn list_provider_api_keys(
    app: AppHandle,
    id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    ProviderService::new(&app).list_api_keys(id).await
}

#[tauri::command]
pub(crate) async fn list_local_provider_api_keys(
    app: AppHandle,
    id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    let task_app = app.clone();
    run_blocking("读取本地 API Key", move || {
        ProviderService::new(&task_app).local_api_keys(id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn add_local_provider_api_key(
    app: AppHandle,
    id: String,
    key: String,
    remark: String,
) -> Result<ProviderView, String> {
    let task_app = app.clone();
    let provider = run_blocking("添加本地 API Key", move || {
        ProviderService::new(&task_app).add_local_api_key(id, key, remark)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(provider.into())
}

#[tauri::command]
pub(crate) async fn set_local_provider_api_key_remark(
    app: AppHandle,
    id: String,
    local_id: String,
    remark: String,
) -> Result<ProviderView, String> {
    let task_app = app.clone();
    let provider = run_blocking("设置 API Key 本地备注", move || {
        ProviderService::new(&task_app).set_local_api_key_remark(id, local_id, remark)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(provider.into())
}

#[tauri::command]
pub(crate) async fn set_default_local_provider_api_key(
    app: AppHandle,
    id: String,
    local_id: String,
) -> Result<ProviderView, String> {
    let task_app = app.clone();
    let provider = run_blocking("设置当前调用 API Key", move || {
        ProviderService::new(&task_app).set_default_local_api_key(id, local_id)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(provider.into())
}

#[tauri::command]
pub(crate) async fn remove_local_provider_api_key(
    app: AppHandle,
    id: String,
    local_id: String,
) -> Result<ProviderView, String> {
    let task_app = app.clone();
    let provider = run_blocking("移除本地 API Key", move || {
        ProviderService::new(&task_app).remove_local_api_key(id, local_id)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(provider.into())
}

#[tauri::command]
pub(crate) async fn create_provider_api_key(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    ProviderService::new(&app).create_api_key(id, name).await
}

#[tauri::command]
pub(crate) async fn create_provider_api_key_for_input(
    app: AppHandle,
    input: ProviderInput,
    name: String,
) -> Result<ProviderApiKeyOption, String> {
    ProviderService::new(&app)
        .create_api_key_for_input(input, name)
        .await
}

#[tauri::command]
pub(crate) async fn generate_provider_access_token_for_input(
    app: AppHandle,
    input: ProviderInput,
) -> Result<String, String> {
    ProviderService::new(&app)
        .generate_access_token_for_input(input)
        .await
}

#[tauri::command]
pub(crate) async fn delete_provider_api_key(
    app: AppHandle,
    id: String,
    token_id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    ProviderService::new(&app)
        .delete_api_key(id, token_id)
        .await
}

#[tauri::command]
pub(crate) async fn get_provider_usage(
    app: AppHandle,
    id: String,
    period: String,
) -> Result<ProviderUsageSummary, String> {
    ProviderService::new(&app).usage_summary(id, period).await
}

#[tauri::command]
pub(crate) async fn get_provider_request_logs(
    app: AppHandle,
    id: String,
    query: ProviderRequestLogsQuery,
) -> Result<ProviderRequestLogsResult, String> {
    ProviderService::new(&app).request_logs(id, query).await
}

#[tauri::command]
pub(crate) async fn change_provider_password(
    app: AppHandle,
    id: String,
    original_password: String,
    password: String,
) -> Result<String, String> {
    ProviderService::new(&app)
        .change_password(id, original_password, password)
        .await
}

#[tauri::command]
pub(crate) async fn get_provider_check_in_records(
    app: AppHandle,
    id: String,
    month: String,
) -> Result<ProviderCheckInRecordsResult, String> {
    ProviderService::new(&app).check_in_records(id, month).await
}

#[tauri::command]
pub(crate) async fn probe_provider_capabilities(
    app: AppHandle,
    id: String,
) -> Result<ProviderCapabilityProbeResultView, String> {
    ProviderService::new(&app)
        .probe_capabilities(id)
        .await
        .map(ProviderCapabilityProbeResultView::from)
}

#[tauri::command]
pub(crate) async fn sync_available_models(
    app: AppHandle,
    id: String,
) -> Result<ProviderModelSyncResultView, String> {
    ProviderService::new(&app)
        .sync_available_models(id)
        .await
        .map(ProviderModelSyncResultView::from)
}

#[tauri::command]
pub(crate) async fn get_provider_invite_link(app: AppHandle, id: String) -> Result<String, String> {
    ProviderService::new(&app).invite_link(id).await
}

#[tauri::command]
pub(crate) async fn get_site_announcements(
    app: AppHandle,
) -> Result<SiteAnnouncementsSnapshot, String> {
    ProviderService::new(&app).site_announcements().await
}

#[tauri::command]
pub(crate) async fn mark_site_announcement_read(
    app: AppHandle,
    provider_id: String,
    announcement_id: String,
) -> Result<(), String> {
    ProviderService::new(&app)
        .mark_site_announcement_read(provider_id, announcement_id)
        .await
}

#[tauri::command]
pub(crate) async fn refresh_all_providers_with_progress(
    app: AppHandle,
    on_event: Channel<ProviderBatchProgressEvent>,
) -> Result<RefreshResultView, String> {
    let result = ProviderService::new(&app)
        .refresh_all_with_progress(on_event)
        .await?;
    tray::refresh_from_state(&app);
    Ok(result.into())
}

#[tauri::command]
pub(crate) async fn refresh_providers(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<RefreshResultView, String> {
    let result = ProviderService::new(&app).refresh_by_ids(ids).await?;
    tray::refresh_from_state(&app);
    Ok(result.into())
}

#[tauri::command]
pub(crate) async fn check_in_provider(
    app: AppHandle,
    id: String,
) -> Result<ProviderCheckInResult, String> {
    ProviderService::new(&app).check_in(id).await
}

#[tauri::command]
pub(crate) async fn check_in_all_providers(
    app: AppHandle,
    on_event: Channel<ProviderBatchProgressEvent>,
) -> Result<RefreshResultView, String> {
    let result = ProviderService::new(&app)
        .check_in_all_with_progress(on_event)
        .await?;
    tray::refresh_from_state(&app);
    Ok(result.into())
}
