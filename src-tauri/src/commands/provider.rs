use crate::{
    contracts::{
        provider_views, ProviderBatchProgressEvent, ProviderBatchResultView,
        ProviderCapabilityProbeResultView, ProviderModelSyncResultView, ProviderSaveResultView,
        ProviderView, RefreshResultView,
    },
    models::{
        ProviderApiKeyEditorContext, ProviderApiKeyOption, ProviderApiKeyPatch,
        ProviderCheckInRecordsResult, ProviderConnectionTestResult,
        ProviderCredentialCompletionResult, ProviderInput, ProviderProtocolDetectionResult,
        ProviderRemovalResult, ProviderRequestLogsQuery, ProviderRequestLogsResult,
        ProviderSaveOptions, ProviderSiteProbeResult, ProviderUsageOverview, ProviderUsageSummary,
        SiteAnnouncementsSnapshot,
    },
    services::provider_service::ProviderService,
    tray,
};
use tauri::{ipc::Channel, AppHandle};
use tauri_plugin_opener::OpenerExt;

use super::run_blocking;

#[tauri::command]
pub(crate) fn preview_provider_check_in_policy(
    input: ProviderInput,
) -> crate::models::provider_domain::check_in::ProviderCheckInPolicyPreview {
    let provider = crate::models::Provider::from_input(input, String::new());
    crate::models::provider_domain::check_in::preview(&provider)
}

#[tauri::command]
pub(crate) async fn open_provider_site(app: AppHandle, id: String) -> Result<(), String> {
    run_blocking("打开中转站", move || {
        let data = ProviderService::new().load_app_data()?;
        let provider = data
            .providers
            .iter()
            .find(|provider| provider.identity.id == id)
            .ok_or("中转站已删除")?;
        let url =
            reqwest::Url::parse(provider.identity.base_url.trim()).map_err(|_| "中转站地址无效")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("仅支持不含账号凭据的 HTTP 或 HTTPS 站点地址".to_string());
        }
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|_| "无法打开系统默认浏览器".to_string())
    })
    .await
}

#[tauri::command]
pub(crate) async fn save_provider(
    app: AppHandle,
    input: ProviderInput,
    options: Option<ProviderSaveOptions>,
) -> Result<ProviderSaveResultView, String> {
    let _task_app = app.clone();
    let result = run_blocking("保存中转站", move || {
        ProviderService::new().save_provider(input, options.unwrap_or_default())
    })
    .await?;
    if result.saved {
        crate::services::check_in_tasks::cancel_outdated();
        tray::refresh_from_state(&app);
    }
    Ok(result.into())
}

#[tauri::command]
pub(crate) async fn set_provider_enabled(
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<ProviderView, String> {
    let provider = ProviderService::new().set_enabled(id, enabled).await?;
    crate::services::check_in_tasks::cancel_outdated();
    tray::refresh_from_state(&app);
    Ok(provider.into())
}

#[tauri::command]
pub(crate) async fn remove_provider(
    app: AppHandle,
    id: String,
) -> Result<ProviderRemovalResult, String> {
    let _task_app = app.clone();
    let result = run_blocking("删除中转站", move || {
        ProviderService::new().remove_provider(id)
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
    let _task_app = app.clone();
    let order = run_blocking("调整中转站顺序", move || {
        ProviderService::new().reorder_providers(ids)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(order)
}

#[tauri::command]
pub(crate) async fn complete_provider_credentials(
    _app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderCredentialCompletionResult, String> {
    ProviderService::new().complete_credentials(input).await
}

#[tauri::command]
pub(crate) async fn test_provider_connection(
    _app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderConnectionTestResult, String> {
    ProviderService::new().test_connection(input).await
}

#[tauri::command]
pub(crate) async fn probe_provider_site(
    _app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderSiteProbeResult, String> {
    ProviderService::new().probe_site(input).await
}

#[tauri::command]
pub(crate) async fn detect_provider_protocol(
    _app: AppHandle,
    input: ProviderInput,
) -> ProviderProtocolDetectionResult {
    ProviderService::new().detect_protocol(input).await
}

#[tauri::command]
pub(crate) async fn list_provider_api_keys(
    _app: AppHandle,
    id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    api_key_request(ProviderService::new().list_api_keys(id), false).await
}

async fn api_key_request<T>(
    operation: impl std::future::Future<Output = Result<T, String>>,
    writing: bool,
) -> Result<T, String> {
    tokio::time::timeout(std::time::Duration::from_secs(60), operation)
        .await
        .map_err(|_| {
            if writing {
                "Key 保存请求超时，站点可能已生效，请先同步确认，避免重复操作"
            } else {
                "读取 Key 信息超时，请重试"
            }
            .to_string()
        })?
}

#[tauri::command]
pub(crate) async fn get_provider_api_key_editor_context(
    _app: AppHandle,
    id: String,
    token_id: Option<String>,
) -> Result<ProviderApiKeyEditorContext, String> {
    api_key_request(
        ProviderService::new().api_key_editor_context(id, token_id),
        false,
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_provider_api_key_editor_context_for_input(
    _app: AppHandle,
    input: ProviderInput,
) -> Result<ProviderApiKeyEditorContext, String> {
    api_key_request(
        ProviderService::new().api_key_editor_context_for_input(input),
        false,
    )
    .await
}

#[tauri::command]
pub(crate) async fn update_provider_api_key(
    _app: AppHandle,
    id: String,
    credential_revision: u64,
    token_id: String,
    patch: ProviderApiKeyPatch,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    api_key_request(
        ProviderService::new().update_api_key(id, credential_revision, token_id, patch),
        true,
    )
    .await
}

#[tauri::command]
pub(crate) async fn list_local_provider_api_keys(
    app: AppHandle,
    id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    let _task_app = app.clone();
    run_blocking("读取本地 API Key", move || {
        ProviderService::new().local_api_keys(id)
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
    let _task_app = app.clone();
    let provider = run_blocking("添加本地 API Key", move || {
        ProviderService::new().add_local_api_key(id, key, remark)
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
    let _task_app = app.clone();
    let provider = run_blocking("设置 API Key 本地备注", move || {
        ProviderService::new().set_local_api_key_remark(id, local_id, remark)
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
    let _task_app = app.clone();
    let provider = run_blocking("设置当前调用 API Key", move || {
        ProviderService::new().set_default_local_api_key(id, local_id)
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
    let _task_app = app.clone();
    let provider = run_blocking("移除本地 API Key", move || {
        ProviderService::new().remove_local_api_key(id, local_id)
    })
    .await?;
    tray::refresh_from_state(&app);
    Ok(provider.into())
}

#[tauri::command]
pub(crate) async fn create_provider_api_key(
    _app: AppHandle,
    id: String,
    credential_revision: u64,
    patch: ProviderApiKeyPatch,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    api_key_request(
        ProviderService::new().create_api_key(id, credential_revision, patch),
        true,
    )
    .await
}

#[tauri::command]
pub(crate) async fn create_provider_api_key_for_input(
    _app: AppHandle,
    input: ProviderInput,
    patch: ProviderApiKeyPatch,
) -> Result<ProviderApiKeyOption, String> {
    api_key_request(
        ProviderService::new().create_api_key_for_input(input, patch),
        true,
    )
    .await
}

#[tauri::command]
pub(crate) async fn generate_provider_access_token_for_input(
    _app: AppHandle,
    input: ProviderInput,
) -> Result<String, String> {
    ProviderService::new()
        .generate_access_token_for_input(input)
        .await
}

#[tauri::command]
pub(crate) async fn delete_provider_api_key(
    _app: AppHandle,
    id: String,
    token_id: String,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    api_key_request(ProviderService::new().delete_api_key(id, token_id), true).await
}

#[tauri::command]
pub(crate) async fn get_provider_usage(
    _app: AppHandle,
    id: String,
    period: String,
) -> Result<ProviderUsageSummary, String> {
    ProviderService::new().usage_summary(id, period).await
}

/// 顶栏的全局用量趋势：不带中转站时聚合全部可读取的站点。
#[tauri::command]
pub(crate) async fn get_usage_overview(
    _app: AppHandle,
    period: String,
) -> Result<ProviderUsageOverview, String> {
    ProviderService::new().usage_summary_overview(&period).await
}

#[tauri::command]
pub(crate) async fn get_provider_request_logs(
    _app: AppHandle,
    id: String,
    query: ProviderRequestLogsQuery,
) -> Result<ProviderRequestLogsResult, String> {
    ProviderService::new().request_logs(id, query).await
}

#[tauri::command]
pub(crate) async fn change_provider_password(
    _app: AppHandle,
    id: String,
    original_password: String,
    password: String,
) -> Result<String, String> {
    ProviderService::new()
        .change_password(id, original_password, password)
        .await
}

#[tauri::command]
pub(crate) async fn get_provider_check_in_records(
    _app: AppHandle,
    id: String,
    month: String,
) -> Result<ProviderCheckInRecordsResult, String> {
    ProviderService::new().check_in_records(id, month).await
}

#[tauri::command]
pub(crate) async fn probe_provider_capabilities(
    _app: AppHandle,
    id: String,
) -> Result<ProviderCapabilityProbeResultView, String> {
    ProviderService::new()
        .probe_capabilities(id)
        .await
        .map(ProviderCapabilityProbeResultView::from)
}

#[tauri::command]
pub(crate) async fn sync_available_models(
    _app: AppHandle,
    id: String,
) -> Result<ProviderModelSyncResultView, String> {
    ProviderService::new()
        .sync_available_models(id)
        .await
        .map(ProviderModelSyncResultView::from)
}

#[tauri::command]
pub(crate) async fn get_provider_invite_link(
    _app: AppHandle,
    id: String,
) -> Result<String, String> {
    ProviderService::new().invite_link(id).await
}

#[tauri::command]
pub(crate) async fn get_site_announcements(
    _app: AppHandle,
) -> Result<SiteAnnouncementsSnapshot, String> {
    ProviderService::new().site_announcements().await
}

#[tauri::command]
pub(crate) async fn mark_site_announcement_read(
    _app: AppHandle,
    provider_id: String,
    announcement_id: String,
) -> Result<(), String> {
    ProviderService::new()
        .mark_site_announcement_read(provider_id, announcement_id)
        .await
}

/// 批次进度走 Tauri IPC 通道；核心只认识 `ProgressSink`。
struct ChannelProgress(Channel<ProviderBatchProgressEvent>);

impl balancehub_core::contracts::ProgressSink for ChannelProgress {
    fn send(&self, event: ProviderBatchProgressEvent) {
        let _ = self.0.send(event);
    }
}

#[tauri::command]
pub(crate) async fn refresh_all_providers_with_progress(
    app: AppHandle,
    on_event: Channel<ProviderBatchProgressEvent>,
) -> Result<ProviderBatchResultView, String> {
    let (result, items) = ProviderService::new()
        .refresh_all_with_progress(std::sync::Arc::new(ChannelProgress(on_event)))
        .await?;
    tray::refresh_from_state(&app);
    Ok(ProviderBatchResultView {
        updated_providers: provider_views(result.updated_providers),
        items,
    })
}

#[tauri::command]
pub(crate) async fn refresh_providers(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<RefreshResultView, String> {
    let result = ProviderService::new().refresh_by_ids(ids).await?;
    tray::refresh_from_state(&app);
    Ok(result.into())
}

#[tauri::command]
pub(crate) fn check_in_provider(
    _app: AppHandle,
    id: String,
) -> Result<crate::models::CheckInTask, String> {
    crate::services::check_in_tasks::enqueue(id, crate::models::CheckInSource::Manual, None, 0)
}

#[tauri::command]
pub(crate) fn list_check_in_tasks(_app: AppHandle) -> Vec<crate::models::CheckInTask> {
    crate::services::check_in_tasks::list()
}

#[tauri::command]
pub(crate) fn cancel_check_in_task(_app: AppHandle, run_id: String) -> Result<(), String> {
    crate::services::check_in_tasks::cancel(&run_id)
}

#[tauri::command]
pub(crate) async fn show_check_in_window(run_id: String) -> Result<(), String> {
    crate::services::check_in_tasks::show_window(&run_id).await
}

#[tauri::command]
pub(crate) fn resume_check_in_task(
    _app: AppHandle,
    run_id: String,
) -> Result<crate::models::CheckInTask, String> {
    crate::services::check_in_tasks::resume(&run_id)
}

#[tauri::command]
pub(crate) fn check_in_all_providers(
    _app: AppHandle,
) -> Result<crate::models::CheckInBatch, String> {
    crate::services::check_in_tasks::enqueue_all()
}
