//! 中转站、额度、用量、日志与 API Key 命令。

use serde_json::Value;
use std::{future::Future, sync::Arc, time::Duration};

use super::{arg, blocking, optional, to_value};
use crate::{
    contracts::{
        provider_views, ProgressSink, ProviderBatchProgressEvent, ProviderBatchResultView,
        ProviderCapabilityProbeResultView, ProviderModelSyncResultView, ProviderSaveResultView,
        ProviderView, RefreshResultView,
    },
    models::{ProviderApiKeyPatch, ProviderInput, ProviderRequestLogsQuery, ProviderSaveOptions},
    services::provider_service::ProviderService,
};

pub const COMMANDS: &[&str] = &[
    "save_provider",
    "get_provider_site_url",
    "remove_provider",
    "reorder_providers",
    "set_provider_enabled",
    "preview_provider_check_in_policy",
    "detect_provider_protocol",
    "probe_provider_site",
    "probe_provider_capabilities",
    "test_provider_connection",
    "sync_available_models",
    "refresh_providers",
    "refresh_all_providers_with_progress",
    "get_site_announcements",
    "mark_site_announcement_read",
    "get_provider_usage",
    "get_usage_overview",
    "get_provider_recommendations",
    "get_provider_request_logs",
    "get_provider_check_in_records",
    "get_provider_invite_link",
    "change_provider_password",
    "complete_provider_credentials",
    "generate_provider_access_token_for_input",
    "create_provider_api_key_for_input",
    "get_provider_api_key_editor_context_for_input",
    "list_provider_api_keys",
    "create_provider_api_key",
    "update_provider_api_key",
    "delete_provider_api_key",
    "get_provider_api_key_editor_context",
    "list_local_provider_api_keys",
    "add_local_provider_api_key",
    "remove_local_provider_api_key",
    "set_default_local_provider_api_key",
    "set_local_provider_api_key_remark",
];

pub async fn handle(
    command: &str,
    args: &Value,
    progress: Option<Arc<dyn ProgressSink>>,
) -> Result<Value, String> {
    let service = ProviderService::new();
    match command {
        "get_provider_site_url" => {
            let id: String = arg(args, "id")?;
            let data = blocking(move || ProviderService::new().load_app_data()).await?;
            let provider = data
                .providers
                .iter()
                .find(|provider| provider.identity.id == id)
                .ok_or("中转站已删除")?;
            let url = reqwest::Url::parse(provider.identity.base_url.trim())
                .map_err(|_| "中转站地址无效")?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("仅支持不含账号凭据的 HTTP 或 HTTPS 站点地址".to_string());
            }
            to_value(url.as_str())
        }
        "save_provider" => {
            let input: ProviderInput = arg(args, "input")?;
            let options: Option<ProviderSaveOptions> = optional(args, "options")?;
            let result =
                blocking(move || service.save_provider(input, options.unwrap_or_default())).await?;
            if result.saved {
                crate::services::check_in_tasks::cancel_outdated();
                crate::host::refresh_tray();
            }
            to_value(ProviderSaveResultView::from(result))
        }
        "remove_provider" => {
            let id: String = arg(args, "id")?;
            let result = blocking(move || service.remove_provider(id)).await?;
            crate::host::refresh_tray();
            to_value(result)
        }
        "reorder_providers" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let order = blocking(move || service.reorder_providers(ids)).await?;
            crate::host::refresh_tray();
            to_value(order)
        }
        "set_provider_enabled" => {
            let id: String = arg(args, "id")?;
            let enabled: bool = arg(args, "enabled")?;
            let provider = service.set_enabled(id, enabled).await?;
            crate::services::check_in_tasks::cancel_outdated();
            crate::host::refresh_tray();
            to_value(ProviderView::from(provider))
        }
        "preview_provider_check_in_policy" => {
            let input: ProviderInput = arg(args, "input")?;
            let provider = crate::models::Provider::from_input(input, String::new());
            to_value(crate::models::provider_domain::check_in::preview(&provider))
        }
        "detect_provider_protocol" => {
            let input: ProviderInput = arg(args, "input")?;
            to_value(service.detect_protocol(input).await)
        }
        "probe_provider_site" => {
            let input: ProviderInput = arg(args, "input")?;
            to_value(service.probe_site(input).await?)
        }
        "probe_provider_capabilities" => {
            let id: String = arg(args, "id")?;
            let result = service.probe_capabilities(id).await?;
            to_value(ProviderCapabilityProbeResultView::from(result))
        }
        "test_provider_connection" => {
            let input: ProviderInput = arg(args, "input")?;
            to_value(service.test_connection(input).await?)
        }
        "sync_available_models" => {
            let id: String = arg(args, "id")?;
            let result = service.sync_available_models(id).await?;
            to_value(ProviderModelSyncResultView::from(result))
        }
        "refresh_providers" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let result = service.refresh_by_ids(ids).await?;
            crate::host::refresh_tray();
            to_value(RefreshResultView::from(result))
        }
        "refresh_all_providers_with_progress" => {
            let sink: Arc<dyn ProgressSink> =
                progress.unwrap_or_else(|| Arc::new(ProgressForwarder));
            let (result, items) = service.refresh_all_with_progress(sink).await?;
            crate::host::refresh_tray();
            to_value(ProviderBatchResultView {
                updated_providers: provider_views(result.updated_providers),
                items,
            })
        }
        "get_site_announcements" => to_value(service.site_announcements().await?),
        "mark_site_announcement_read" => {
            let provider_id: String = arg(args, "providerId")?;
            let announcement_id: String = arg(args, "announcementId")?;
            service
                .mark_site_announcement_read(provider_id, announcement_id)
                .await?;
            Ok(Value::Null)
        }
        "get_provider_usage" => {
            let id: String = arg(args, "id")?;
            let period: String = arg(args, "period")?;
            to_value(service.usage_summary(id, period).await?)
        }
        "get_usage_overview" => {
            let period: String = arg(args, "period")?;
            to_value(service.usage_summary_overview(&period).await?)
        }
        "get_provider_recommendations" => {
            let model: Option<String> = optional(args, "model")?;
            let data = blocking(|| ProviderService::new().load_app_data()).await?;
            to_value(crate::services::recommendation::recommend(
                &data.providers,
                model.as_deref().unwrap_or_default(),
                chrono::Utc::now(),
            ))
        }
        "get_provider_request_logs" => {
            let id: String = arg(args, "id")?;
            let query: ProviderRequestLogsQuery = arg(args, "query")?;
            to_value(service.request_logs(id, query).await?)
        }
        "get_provider_check_in_records" => {
            let id: String = arg(args, "id")?;
            let month: String = arg(args, "month")?;
            to_value(service.check_in_records(id, month).await?)
        }
        "get_provider_invite_link" => {
            let id: String = arg(args, "id")?;
            to_value(service.invite_link(id).await?)
        }
        "change_provider_password" => {
            let id: String = arg(args, "id")?;
            let original: String = arg(args, "originalPassword")?;
            let password: String = arg(args, "password")?;
            to_value(service.change_password(id, original, password).await?)
        }
        "complete_provider_credentials" => {
            let input: ProviderInput = arg(args, "input")?;
            to_value(service.complete_credentials(input).await?)
        }
        "generate_provider_access_token_for_input" => {
            let input: ProviderInput = arg(args, "input")?;
            to_value(service.generate_access_token_for_input(input).await?)
        }
        "create_provider_api_key_for_input" => {
            let input: ProviderInput = arg(args, "input")?;
            let patch: ProviderApiKeyPatch = arg(args, "patch")?;
            let result = key_request(true, service.create_api_key_for_input(input, patch)).await?;
            to_value(result)
        }
        "get_provider_api_key_editor_context_for_input" => {
            let input: ProviderInput = arg(args, "input")?;
            let result =
                key_request(false, service.api_key_editor_context_for_input(input)).await?;
            to_value(result)
        }
        "list_provider_api_keys" => {
            let id: String = arg(args, "id")?;
            let result = key_request(false, service.list_api_keys(id)).await?;
            to_value(result)
        }
        "create_provider_api_key" => {
            let id: String = arg(args, "id")?;
            let revision: u64 = arg(args, "credentialRevision")?;
            let patch: ProviderApiKeyPatch = arg(args, "patch")?;
            let result = key_request(true, service.create_api_key(id, revision, patch)).await?;
            to_value(result)
        }
        "update_provider_api_key" => {
            let id: String = arg(args, "id")?;
            let revision: u64 = arg(args, "credentialRevision")?;
            let token_id: String = arg(args, "tokenId")?;
            let patch: ProviderApiKeyPatch = arg(args, "patch")?;
            let result =
                key_request(true, service.update_api_key(id, revision, token_id, patch)).await?;
            to_value(result)
        }
        "delete_provider_api_key" => {
            let id: String = arg(args, "id")?;
            let token_id: String = arg(args, "tokenId")?;
            let result = key_request(true, service.delete_api_key(id, token_id)).await?;
            to_value(result)
        }
        "get_provider_api_key_editor_context" => {
            let id: String = arg(args, "id")?;
            let token_id: Option<String> = optional(args, "tokenId")?;
            let result = key_request(false, service.api_key_editor_context(id, token_id)).await?;
            to_value(result)
        }
        "list_local_provider_api_keys" => {
            let id: String = arg(args, "id")?;
            let result = blocking(move || service.local_api_keys(id)).await?;
            to_value(result)
        }
        "add_local_provider_api_key" => {
            let id: String = arg(args, "id")?;
            let key: String = arg(args, "key")?;
            let remark: String = arg(args, "remark")?;
            let provider = blocking(move || service.add_local_api_key(id, key, remark)).await?;
            to_value(ProviderView::from(provider))
        }
        "remove_local_provider_api_key" => {
            let id: String = arg(args, "id")?;
            let local_id: String = arg(args, "localId")?;
            let provider = blocking(move || service.remove_local_api_key(id, local_id)).await?;
            to_value(ProviderView::from(provider))
        }
        "set_default_local_provider_api_key" => {
            let id: String = arg(args, "id")?;
            let local_id: String = arg(args, "localId")?;
            let provider =
                blocking(move || service.set_default_local_api_key(id, local_id)).await?;
            to_value(ProviderView::from(provider))
        }
        "set_local_provider_api_key_remark" => {
            let id: String = arg(args, "id")?;
            let local_id: String = arg(args, "localId")?;
            let remark: String = arg(args, "remark")?;
            let provider =
                blocking(move || service.set_local_api_key_remark(id, local_id, remark)).await?;
            to_value(ProviderView::from(provider))
        }
        _ => Err(format!("未实现的中转站命令：{command}")),
    }
}

/// 站点 Key 操作可能被站点拖住；写入类超时要提示可能已生效，避免重复创建。
const KEY_OPERATION_TIMEOUT: Duration = Duration::from_secs(60);

async fn key_request<T>(
    writing: bool,
    future: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(KEY_OPERATION_TIMEOUT, future)
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

/// 刷新批次进度：服务端把它转成事件推给浏览器。
pub(crate) struct ProgressForwarder;

impl ProgressSink for ProgressForwarder {
    fn send(&self, event: ProviderBatchProgressEvent) {
        crate::host::emit("provider-batch-progress", event);
    }
}
