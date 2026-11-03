//! 中转站、额度、用量、日志、API Key 与签到命令。
//!
//! 业务实现全部来自 `balancehub_core::api`，这里只保留桌面专属的
//! 「用系统浏览器打开站点」和 Tauri 进度通道的装配。

use crate::{
    contracts::ProviderBatchProgressEvent,
    models::{ProviderApiKeyPatch, ProviderInput, ProviderRequestLogsQuery, ProviderSaveOptions},
    services::provider_service::ProviderService,
};
use serde_json::json;
use tauri::{ipc::Channel, AppHandle};
use tauri_plugin_opener::OpenerExt;

use super::{dispatch_value, dispatch_value_with_progress};

#[tauri::command]
pub(crate) async fn open_provider_site(app: AppHandle, id: String) -> Result<(), String> {
    let data = super::run_blocking("读取中转站", move || {
        ProviderService::new().load_app_data()
    })
    .await?;
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
}

#[tauri::command]
pub(crate) async fn preview_provider_check_in_policy(
    input: ProviderInput,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "preview_provider_check_in_policy",
        json!({ "input": input }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn save_provider(
    input: ProviderInput,
    options: Option<ProviderSaveOptions>,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "save_provider",
        json!({ "input": input, "options": options }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_provider_enabled(
    id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "set_provider_enabled",
        json!({ "id": id, "enabled": enabled }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn remove_provider(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("remove_provider", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn reorder_providers(ids: Vec<String>) -> Result<serde_json::Value, String> {
    dispatch_value("reorder_providers", json!({ "ids": ids })).await
}

#[tauri::command]
pub(crate) async fn complete_provider_credentials(
    input: ProviderInput,
) -> Result<serde_json::Value, String> {
    dispatch_value("complete_provider_credentials", json!({ "input": input })).await
}

#[tauri::command]
pub(crate) async fn test_provider_connection(
    input: ProviderInput,
) -> Result<serde_json::Value, String> {
    dispatch_value("test_provider_connection", json!({ "input": input })).await
}

#[tauri::command]
pub(crate) async fn probe_provider_site(input: ProviderInput) -> Result<serde_json::Value, String> {
    dispatch_value("probe_provider_site", json!({ "input": input })).await
}

#[tauri::command]
pub(crate) async fn detect_provider_protocol(
    input: ProviderInput,
) -> Result<serde_json::Value, String> {
    dispatch_value("detect_provider_protocol", json!({ "input": input })).await
}

#[tauri::command]
pub(crate) async fn list_provider_api_keys(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("list_provider_api_keys", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn get_provider_api_key_editor_context(
    id: String,
    token_id: Option<String>,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "get_provider_api_key_editor_context",
        json!({ "id": id, "tokenId": token_id }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_provider_api_key_editor_context_for_input(
    input: ProviderInput,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "get_provider_api_key_editor_context_for_input",
        json!({ "input": input }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn update_provider_api_key(
    id: String,
    credential_revision: u64,
    token_id: String,
    patch: ProviderApiKeyPatch,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "update_provider_api_key",
        json!({
            "id": id,
            "credentialRevision": credential_revision,
            "tokenId": token_id,
            "patch": patch,
        }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn list_local_provider_api_keys(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("list_local_provider_api_keys", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn add_local_provider_api_key(
    id: String,
    key: String,
    remark: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "add_local_provider_api_key",
        json!({ "id": id, "key": key, "remark": remark }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_local_provider_api_key_remark(
    id: String,
    local_id: String,
    remark: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "set_local_provider_api_key_remark",
        json!({ "id": id, "localId": local_id, "remark": remark }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_default_local_provider_api_key(
    id: String,
    local_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "set_default_local_provider_api_key",
        json!({ "id": id, "localId": local_id }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn remove_local_provider_api_key(
    id: String,
    local_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "remove_local_provider_api_key",
        json!({ "id": id, "localId": local_id }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn create_provider_api_key(
    id: String,
    credential_revision: u64,
    patch: ProviderApiKeyPatch,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "create_provider_api_key",
        json!({ "id": id, "credentialRevision": credential_revision, "patch": patch }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn create_provider_api_key_for_input(
    input: ProviderInput,
    patch: ProviderApiKeyPatch,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "create_provider_api_key_for_input",
        json!({ "input": input, "patch": patch }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn generate_provider_access_token_for_input(
    input: ProviderInput,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "generate_provider_access_token_for_input",
        json!({ "input": input }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn delete_provider_api_key(
    id: String,
    token_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "delete_provider_api_key",
        json!({ "id": id, "tokenId": token_id }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_provider_usage(
    id: String,
    period: String,
) -> Result<serde_json::Value, String> {
    dispatch_value("get_provider_usage", json!({ "id": id, "period": period })).await
}

#[tauri::command]
pub(crate) async fn get_usage_overview(period: String) -> Result<serde_json::Value, String> {
    dispatch_value("get_usage_overview", json!({ "period": period })).await
}

#[tauri::command]
pub(crate) async fn get_provider_request_logs(
    id: String,
    query: ProviderRequestLogsQuery,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "get_provider_request_logs",
        json!({ "id": id, "query": query }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn change_provider_password(
    id: String,
    original_password: String,
    password: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "change_provider_password",
        json!({
            "id": id,
            "originalPassword": original_password,
            "password": password,
        }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_provider_check_in_records(
    id: String,
    month: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "get_provider_check_in_records",
        json!({ "id": id, "month": month }),
    )
    .await
}

#[tauri::command]
pub(crate) async fn probe_provider_capabilities(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("probe_provider_capabilities", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn sync_available_models(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("sync_available_models", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn get_provider_invite_link(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("get_provider_invite_link", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn get_site_announcements() -> Result<serde_json::Value, String> {
    dispatch_value("get_site_announcements", json!({})).await
}

#[tauri::command]
pub(crate) async fn mark_site_announcement_read(
    provider_id: String,
    announcement_id: String,
) -> Result<serde_json::Value, String> {
    dispatch_value(
        "mark_site_announcement_read",
        json!({ "providerId": provider_id, "announcementId": announcement_id }),
    )
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
    on_event: Channel<ProviderBatchProgressEvent>,
) -> Result<serde_json::Value, String> {
    dispatch_value_with_progress(
        "refresh_all_providers_with_progress",
        json!({}),
        Some(std::sync::Arc::new(ChannelProgress(on_event))),
    )
    .await
}

#[tauri::command]
pub(crate) async fn refresh_providers(ids: Vec<String>) -> Result<serde_json::Value, String> {
    dispatch_value("refresh_providers", json!({ "ids": ids })).await
}

#[tauri::command]
pub(crate) async fn check_in_provider(id: String) -> Result<serde_json::Value, String> {
    dispatch_value("check_in_provider", json!({ "id": id })).await
}

#[tauri::command]
pub(crate) async fn list_check_in_tasks() -> Result<serde_json::Value, String> {
    dispatch_value("list_check_in_tasks", json!({})).await
}

#[tauri::command]
pub(crate) async fn cancel_check_in_task(run_id: String) -> Result<serde_json::Value, String> {
    dispatch_value("cancel_check_in_task", json!({ "runId": run_id })).await
}

#[tauri::command]
pub(crate) async fn show_check_in_window(run_id: String) -> Result<serde_json::Value, String> {
    dispatch_value("show_check_in_window", json!({ "runId": run_id })).await
}

#[tauri::command]
pub(crate) async fn resume_check_in_task(run_id: String) -> Result<serde_json::Value, String> {
    dispatch_value("resume_check_in_task", json!({ "runId": run_id })).await
}

#[tauri::command]
pub(crate) async fn check_in_all_providers() -> Result<serde_json::Value, String> {
    dispatch_value("check_in_all_providers", json!({})).await
}
