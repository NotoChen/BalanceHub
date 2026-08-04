use crate::{
    contracts::{
        provider_views, CodexModelSyncResultView, ProviderCapabilityProbeResultView, ProviderView,
        RefreshResultView,
    },
    models::{
        ProviderApiKeyOption, ProviderCheckInRecordsResult, ProviderCheckInResult,
        ProviderConnectionTestResult, ProviderCredentialCompletionResult, ProviderInput,
        ProviderProtocolDetectionResult, ProviderRequestLogsQuery, ProviderRequestLogsResult,
        ProviderSiteProbeResult, ProviderUsageSummary,
    },
    services::provider_service::ProviderService,
    tray,
};
use tauri::AppHandle;

#[tauri::command]
pub(crate) fn save_provider(
    app: AppHandle,
    input: ProviderInput,
) -> Result<Vec<ProviderView>, String> {
    let providers = ProviderService::new(&app).save_provider(input)?;
    tray::refresh_from_state(&app);
    Ok(provider_views(providers))
}

#[tauri::command]
pub(crate) fn remove_provider(app: AppHandle, id: String) -> Result<Vec<ProviderView>, String> {
    let providers = ProviderService::new(&app).remove_provider(id)?;
    tray::refresh_from_state(&app);
    Ok(provider_views(providers))
}

#[tauri::command]
pub(crate) fn reorder_providers(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<ProviderView>, String> {
    let providers = ProviderService::new(&app).reorder_providers(ids)?;
    tray::refresh_from_state(&app);
    Ok(provider_views(providers))
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
pub(crate) async fn sync_codex_models(
    app: AppHandle,
    id: String,
) -> Result<CodexModelSyncResultView, String> {
    ProviderService::new(&app)
        .sync_codex_models(id)
        .await
        .map(CodexModelSyncResultView::from)
}

#[tauri::command]
pub(crate) async fn get_provider_invite_link(app: AppHandle, id: String) -> Result<String, String> {
    ProviderService::new(&app).invite_link(id).await
}

#[tauri::command]
pub(crate) async fn refresh_all_providers(app: AppHandle) -> Result<RefreshResultView, String> {
    let result = ProviderService::new(&app).refresh_all().await?;
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
pub(crate) async fn pass_provider_challenge(app: AppHandle, id: String) -> Result<String, String> {
    ProviderService::new(&app).pass_challenge(id).await
}
