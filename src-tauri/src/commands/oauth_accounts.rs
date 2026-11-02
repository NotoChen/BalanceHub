use crate::{
    contracts::{provider_views, OAuthAccountDetails, OAuthExportResult, ProviderView},
    models::*,
    services::{
        oauth_accounts::{imports, login},
        provider_service::ProviderService,
    },
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::run_blocking;

#[tauri::command]
pub(crate) async fn start_oauth_login(
    _app: AppHandle,
    vendor: OAuthVendor,
    provider_id: Option<String>,
) -> Result<OAuthLoginTask, String> {
    login::start(vendor, provider_id).await
}
#[tauri::command]
pub(crate) fn list_oauth_logins() -> Vec<OAuthLoginTask> {
    login::list()
}
#[tauri::command]
pub(crate) fn cancel_oauth_login(id: String) -> Result<(), String> {
    login::cancel(&id)
}
#[tauri::command]
pub(crate) fn submit_oauth_callback(id: String, value: String) -> Result<(), String> {
    login::submit(&id, &value)
}
#[tauri::command]
pub(crate) fn retry_oauth_login_save(_app: AppHandle, id: String) -> Result<(), String> {
    login::retry_save(id)
}
#[tauri::command]
pub(crate) async fn open_oauth_login_page(_app: AppHandle, id: String) -> Result<(), String> {
    login::reopen(id).await
}

#[tauri::command]
pub(crate) async fn preview_oauth_import(
    _app: AppHandle,
    files: Vec<OAuthImportFile>,
    paths: Vec<String>,
    vendor: Option<OAuthVendor>,
) -> Result<OAuthImportPreview, String> {
    super::run_blocking("读取授权文件", move || {
        if files.len() + paths.len() > 64 {
            return Err("一次最多选择 64 个授权文件".into());
        }
        let data = ProviderService::new().load_app_data()?;
        let mut files = files;
        for path in paths {
            let path = std::path::Path::new(&path);
            let content =
                crate::util::read_text_file_limited(path, 4 * 1024 * 1024, "读取授权文件")?;
            files.push(OAuthImportFile {
                name: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("auth.json")
                    .into(),
                content,
            });
        }
        imports::prepare(files, vendor, &data.providers)
    })
    .await
}
#[tauri::command]
pub(crate) fn discard_oauth_import(token: String) {
    imports::discard(&token);
}
#[tauri::command]
pub(crate) async fn apply_oauth_import(
    _app: AppHandle,
    request: OAuthImportRequest,
) -> Result<Vec<ProviderView>, String> {
    Ok(provider_views(
        ProviderService::new()
            .import_oauth_accounts(request)
            .await?,
    ))
}
#[tauri::command]
pub(crate) async fn get_oauth_account(
    _app: AppHandle,
    id: String,
) -> Result<OAuthAccountDetails, String> {
    ProviderService::new().oauth_details(id).await
}
#[tauri::command]
pub(crate) async fn set_oauth_renewal(
    _app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<ProviderView, String> {
    Ok(ProviderService::new()
        .set_oauth_renewal(id, enabled)
        .await?
        .into())
}
#[tauri::command]
pub(crate) async fn update_oauth_account(
    _app: AppHandle,
    id: String,
    name: String,
    remark: String,
    refresh_interval: u64,
) -> Result<ProviderView, String> {
    Ok(ProviderService::new()
        .update_oauth_metadata(id, name, remark, refresh_interval)
        .await?
        .into())
}
#[tauri::command]
pub(crate) async fn get_oauth_credential(_app: AppHandle, id: String) -> Result<String, String> {
    ProviderService::new().oauth_credential_source(id).await
}
#[tauri::command]
pub(crate) async fn replace_oauth_credential(
    _app: AppHandle,
    id: String,
    content: String,
    revision: u64,
) -> Result<ProviderView, String> {
    Ok(ProviderService::new()
        .replace_oauth_credential(id, content, revision)
        .await?
        .into())
}
#[tauri::command]
pub(crate) async fn export_oauth_account(
    _app: AppHandle,
    id: String,
    format: OAuthExportFormat,
    path: String,
    pause_renewal: bool,
) -> Result<OAuthExportResult, String> {
    ProviderService::new()
        .export_oauth_file(id, format, path, pause_renewal)
        .await
}
#[tauri::command]
pub(crate) async fn preview_cpa_oauth_import(
    _app: AppHandle,
    id: String,
    names: Vec<String>,
) -> Result<OAuthImportPreview, String> {
    ProviderService::new().import_cpa_preview(id, names).await
}
#[tauri::command]
pub(crate) async fn send_oauth_to_cpa(
    _app: AppHandle,
    id: String,
    gateway_id: String,
) -> Result<OAuthExportResult, String> {
    ProviderService::new()
        .transfer_oauth_to_cpa(id, gateway_id)
        .await
}
#[tauri::command]
pub(crate) async fn set_cpa_oauth_disabled(
    _app: AppHandle,
    id: String,
    name: String,
    auth_index: String,
    disabled: bool,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .set_cpa_account_disabled(id, name, auth_index, disabled, revision)
        .await
}
#[tauri::command]
pub(crate) async fn update_cpa_oauth_fields(
    _app: AppHandle,
    id: String,
    name: String,
    auth_index: String,
    fields: CpaCredentialFields,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .set_cpa_account_fields(id, name, auth_index, fields, revision)
        .await
}
#[tauri::command]
pub(crate) async fn refresh_cpa_oauth(
    _app: AppHandle,
    id: String,
    name: String,
    auth_index: String,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .refresh_cpa_account(id, name, auth_index, revision)
        .await
}
#[tauri::command]
pub(crate) async fn reset_cpa_oauth_cooldown(
    _app: AppHandle,
    id: String,
    name: String,
    auth_index: String,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .reset_cpa_account_cooldown(id, name, auth_index, revision)
        .await
}
#[tauri::command]
pub(crate) async fn refresh_all_cpa_oauth(
    _app: AppHandle,
    id: String,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .refresh_all_cpa_accounts(id, revision)
        .await
}
#[tauri::command]
pub(crate) async fn list_cpa_error_logs(
    _app: AppHandle,
    id: String,
) -> Result<Vec<CpaErrorLog>, String> {
    ProviderService::new().cpa_error_logs(id).await
}
#[tauri::command]
pub(crate) async fn list_cpa_plugins(_app: AppHandle, id: String) -> Result<CpaPluginList, String> {
    ProviderService::new().cpa_plugins(id).await
}
#[tauri::command]
pub(crate) async fn list_cpa_plugin_store(
    _app: AppHandle,
    id: String,
) -> Result<CpaPluginStore, String> {
    ProviderService::new().cpa_plugin_store(id).await
}
#[tauri::command]
pub(crate) async fn install_cpa_plugin(
    _app: AppHandle,
    id: String,
    plugin_id: String,
    source_id: String,
    version: String,
    revision: u64,
) -> Result<CpaStoreInstall, String> {
    ProviderService::new()
        .install_cpa_plugin(id, plugin_id, source_id, version, revision)
        .await
}
#[tauri::command]
pub(crate) async fn check_cpa_version(
    _app: AppHandle,
    id: String,
) -> Result<CpaVersionCheck, String> {
    ProviderService::new().check_cpa_version(id).await
}
#[tauri::command]
pub(crate) async fn get_cpa_plugin_config(
    _app: AppHandle,
    id: String,
    plugin_id: String,
) -> Result<CpaPluginConfig, String> {
    ProviderService::new()
        .cpa_plugin_config(id, plugin_id)
        .await
}
#[tauri::command]
pub(crate) async fn set_cpa_plugin_enabled(
    _app: AppHandle,
    id: String,
    plugin_id: String,
    enabled: bool,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .set_cpa_plugin_enabled(id, plugin_id, enabled, revision)
        .await
}
#[tauri::command]
pub(crate) async fn update_cpa_plugin_config(
    _app: AppHandle,
    id: String,
    plugin_id: String,
    content: String,
    revision: u64,
) -> Result<CpaPluginConfig, String> {
    ProviderService::new()
        .update_cpa_plugin_config(id, plugin_id, content, revision)
        .await
}
#[tauri::command]
pub(crate) async fn remove_cpa_plugin(
    _app: AppHandle,
    id: String,
    plugin_id: String,
    revision: u64,
) -> Result<CpaPluginRemoval, String> {
    ProviderService::new()
        .remove_cpa_plugin(id, plugin_id, revision)
        .await
}
#[tauri::command]
pub(crate) async fn list_cpa_login_providers(
    _app: AppHandle,
    id: String,
) -> Result<Vec<CpaLoginProvider>, String> {
    ProviderService::new().cpa_login_providers(id).await
}
#[tauri::command]
pub(crate) async fn start_cpa_login(
    _app: AppHandle,
    id: String,
    provider: String,
) -> Result<CpaLoginStart, String> {
    ProviderService::new().start_cpa_login(id, provider).await
}
#[tauri::command]
pub(crate) async fn get_cpa_login_status(
    _app: AppHandle,
    id: String,
    state: String,
    api_version: String,
) -> Result<CpaLoginStatus, String> {
    ProviderService::new()
        .cpa_login_status(id, state, api_version)
        .await
}
#[tauri::command]
pub(crate) async fn submit_cpa_login_callback(
    _app: AppHandle,
    id: String,
    provider: String,
    state: String,
    api_version: String,
    input: String,
) -> Result<(), String> {
    ProviderService::new()
        .submit_cpa_login_callback(id, provider, state, api_version, input)
        .await
}
#[tauri::command]
pub(crate) async fn cancel_cpa_login(
    _app: AppHandle,
    id: String,
    state: String,
) -> Result<(), String> {
    ProviderService::new().cancel_cpa_login(id, state).await
}
#[tauri::command]
pub(crate) async fn get_cpa_account_quota(
    _app: AppHandle,
    id: String,
    auth_index: String,
    plugin_id: String,
    api_version: String,
) -> Result<CpaAccountQuota, String> {
    ProviderService::new()
        .cpa_account_quota(id, auth_index, plugin_id, api_version)
        .await
}
/// Opens the URL the gateway generated. It is held only in the panel, so it is
/// checked here before the system browser sees it.
#[tauri::command]
pub(crate) async fn open_cpa_login_url(
    app: AppHandle,
    id: String,
    url: String,
) -> Result<(), String> {
    run_blocking("打开授权页面", move || {
        let provider = ProviderService::new()
            .load_app_data()?
            .providers
            .into_iter()
            .find(|provider| provider.identity.id == id)
            .ok_or("CPA 网关已删除")?;
        if provider.identity.protocol != ProviderProtocol::CliProxyApi {
            return Err("所选卡片不是 CPA 网关".to_string());
        }
        let parsed = reqwest::Url::parse(url.trim()).map_err(|_| "授权地址无效")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("授权地址必须是完整的 HTTP 或 HTTPS 链接".to_string());
        }
        app.opener()
            .open_url(parsed.as_str(), None::<&str>)
            .map_err(|_| "无法打开系统默认浏览器".to_string())
    })
    .await
}
#[tauri::command]
pub(crate) async fn get_cpa_model_rules(
    _app: AppHandle,
    id: String,
) -> Result<CpaModelRules, String> {
    ProviderService::new().cpa_model_rules(id).await
}
#[tauri::command]
pub(crate) async fn get_cpa_model_definitions(
    _app: AppHandle,
    id: String,
    channel: String,
) -> Result<Vec<CpaModelDefinition>, String> {
    ProviderService::new()
        .cpa_model_definitions(id, channel)
        .await
}
#[tauri::command]
pub(crate) async fn update_cpa_model_rules(
    _app: AppHandle,
    id: String,
    kind: CpaModelRuleKind,
    content: String,
    revision: u64,
) -> Result<CpaModelRules, String> {
    ProviderService::new()
        .update_cpa_model_rules(id, kind, content, revision)
        .await
}
#[tauri::command]
pub(crate) async fn get_cpa_gateway_settings(
    _app: AppHandle,
    id: String,
) -> Result<CpaGatewaySettings, String> {
    Ok(ProviderService::new().cpa_gateway_settings(id).await?.0)
}
#[tauri::command]
pub(crate) async fn update_cpa_gateway_settings(
    _app: AppHandle,
    id: String,
    update: CpaGatewaySettingsUpdate,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .update_cpa_gateway_settings(id, update, revision)
        .await
}
#[tauri::command]
pub(crate) async fn list_cpa_client_keys(
    _app: AppHandle,
    id: String,
) -> Result<CpaClientKeys, String> {
    ProviderService::new().cpa_client_keys(id).await
}
#[tauri::command]
pub(crate) async fn add_cpa_client_key(
    _app: AppHandle,
    id: String,
    input: CpaClientKeyAdd,
    revision: u64,
) -> Result<CpaClientKeyAdded, String> {
    ProviderService::new()
        .add_cpa_client_key(id, input, revision)
        .await
}
#[tauri::command]
pub(crate) async fn remove_cpa_client_key(
    _app: AppHandle,
    id: String,
    value: String,
    revision: u64,
) -> Result<CpaClientKeys, String> {
    ProviderService::new()
        .remove_cpa_client_key(id, value, revision)
        .await
}
#[tauri::command]
pub(crate) async fn read_cpa_request_logs(
    _app: AppHandle,
    id: String,
    limit: u32,
    cursor: String,
) -> Result<CpaLogTail, String> {
    ProviderService::new()
        .cpa_request_logs(id, limit, cursor)
        .await
}
#[tauri::command]
pub(crate) async fn read_cpa_api_key_usage(
    _app: AppHandle,
    id: String,
) -> Result<Vec<CpaApiKeyUsage>, String> {
    ProviderService::new().cpa_api_key_usage(id).await
}
#[tauri::command]
pub(crate) async fn read_cpa_request_log(
    _app: AppHandle,
    id: String,
    request_id: String,
) -> Result<CpaRequestLog, String> {
    ProviderService::new().cpa_request_log(id, request_id).await
}
#[tauri::command]
pub(crate) async fn clear_cpa_logs(
    _app: AppHandle,
    id: String,
    revision: u64,
) -> Result<CpaLogClear, String> {
    ProviderService::new().clear_cpa_logs(id, revision).await
}
#[tauri::command]
pub(crate) async fn read_cpa_gateway_config(
    _app: AppHandle,
    id: String,
) -> Result<CpaGatewayConfig, String> {
    ProviderService::new().cpa_gateway_config(id).await
}
#[tauri::command]
pub(crate) async fn update_cpa_gateway_config(
    _app: AppHandle,
    id: String,
    content: String,
    etag: String,
    api_version: String,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .update_cpa_gateway_config(id, content, etag, api_version, revision)
        .await
}
#[tauri::command]
pub(crate) async fn get_cpa_log_settings(
    _app: AppHandle,
    id: String,
) -> Result<CpaLogSettings, String> {
    ProviderService::new().cpa_log_settings(id).await
}
#[tauri::command]
pub(crate) async fn set_cpa_log_settings(
    _app: AppHandle,
    id: String,
    enabled: bool,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .set_cpa_log_settings(id, enabled, revision)
        .await
}
#[tauri::command]
pub(crate) async fn read_cpa_error_log(
    _app: AppHandle,
    id: String,
    name: String,
) -> Result<CpaErrorLogContent, String> {
    ProviderService::new().read_cpa_error_log(id, name).await
}
#[tauri::command]
pub(crate) async fn delete_cpa_oauth(
    _app: AppHandle,
    id: String,
    name: String,
    auth_index: String,
    revision: u64,
) -> Result<(), String> {
    ProviderService::new()
        .delete_cpa_account(id, name, auth_index, revision)
        .await
}
#[tauri::command]
pub(crate) async fn view_cpa_oauth_file(
    _app: AppHandle,
    id: String,
    name: String,
) -> Result<CpaCredentialFile, String> {
    ProviderService::new().read_cpa_account_file(id, name).await
}
