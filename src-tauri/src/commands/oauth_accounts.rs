//! OAuth 账号与 CPA 网关命令。
//!
//! 业务实现全部来自 `balancehub_core::api`。桌面只保留三件宿主相关的事：
//! 打开授权页面（系统浏览器）、选择本地导出路径，以及把结果原样回给前端。

use super::dispatch_value;
use crate::{
    contracts::OAuthExportResult,
    models::*,
    services::{oauth_accounts::login, provider_service::ProviderService},
};
use serde_json::{json, Value};

#[tauri::command]
pub(crate) async fn start_oauth_login(
    vendor: OAuthVendor,
    provider_id: Option<String>,
) -> Result<Value, String> {
    dispatch_value(
        "start_oauth_login",
        json!({ "vendor": vendor, "providerId": provider_id }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn list_oauth_logins() -> Result<Value, String> {
    dispatch_value("list_oauth_logins", json!({})).await
}
#[tauri::command]
pub(crate) async fn cancel_oauth_login(id: String) -> Result<Value, String> {
    dispatch_value("cancel_oauth_login", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn submit_oauth_callback(id: String, value: String) -> Result<Value, String> {
    dispatch_value("submit_oauth_callback", json!({ "id": id, "value": value })).await
}
#[tauri::command]
pub(crate) async fn retry_oauth_login_save(id: String) -> Result<Value, String> {
    dispatch_value("retry_oauth_login_save", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn open_oauth_login_page(id: String) -> Result<(), String> {
    login::reopen(id).await
}

#[tauri::command]
pub(crate) async fn preview_oauth_import(
    files: Vec<OAuthImportFile>,
    paths: Vec<String>,
    vendor: Option<OAuthVendor>,
) -> Result<Value, String> {
    dispatch_value(
        "preview_oauth_import",
        json!({ "files": files, "paths": paths, "vendor": vendor }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn discard_oauth_import(token: String) -> Result<Value, String> {
    dispatch_value("discard_oauth_import", json!({ "token": token })).await
}
#[tauri::command]
pub(crate) async fn apply_oauth_import(request: OAuthImportRequest) -> Result<Value, String> {
    dispatch_value("apply_oauth_import", json!({ "request": request })).await
}
#[tauri::command]
pub(crate) async fn get_oauth_account(id: String) -> Result<Value, String> {
    dispatch_value("get_oauth_account", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn set_oauth_renewal(id: String, enabled: bool) -> Result<Value, String> {
    dispatch_value("set_oauth_renewal", json!({ "id": id, "enabled": enabled })).await
}
#[tauri::command]
pub(crate) async fn update_oauth_account(
    id: String,
    name: String,
    remark: String,
    refresh_interval: u64,
) -> Result<Value, String> {
    dispatch_value(
        "update_oauth_account",
        json!({ "id": id, "name": name, "remark": remark, "refreshInterval": refresh_interval }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn get_oauth_credential(id: String) -> Result<Value, String> {
    dispatch_value("get_oauth_credential", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn replace_oauth_credential(
    id: String,
    content: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "replace_oauth_credential",
        json!({ "id": id, "content": content, "revision": revision }),
    )
    .await
}
/// 桌面导出到用户选择的文件；浏览器版改用 `export_oauth_account_text`。
#[tauri::command]
pub(crate) async fn export_oauth_account(
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
    id: String,
    names: Vec<String>,
) -> Result<Value, String> {
    dispatch_value(
        "preview_cpa_oauth_import",
        json!({ "id": id, "names": names }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn send_oauth_to_cpa(id: String, gateway_id: String) -> Result<Value, String> {
    dispatch_value(
        "send_oauth_to_cpa",
        json!({ "id": id, "gatewayId": gateway_id }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn set_cpa_oauth_disabled(
    id: String,
    name: String,
    auth_index: String,
    disabled: bool,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "set_cpa_oauth_disabled",
        json!({ "id": id, "name": name, "authIndex": auth_index, "disabled": disabled, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn update_cpa_oauth_fields(
    id: String,
    name: String,
    auth_index: String,
    fields: CpaCredentialFields,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "update_cpa_oauth_fields",
        json!({ "id": id, "name": name, "authIndex": auth_index, "fields": fields, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn refresh_cpa_oauth(
    id: String,
    name: String,
    auth_index: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "refresh_cpa_oauth",
        json!({ "id": id, "name": name, "authIndex": auth_index, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn reset_cpa_oauth_cooldown(
    id: String,
    name: String,
    auth_index: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "reset_cpa_oauth_cooldown",
        json!({ "id": id, "name": name, "authIndex": auth_index, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn refresh_all_cpa_oauth(id: String, revision: u64) -> Result<Value, String> {
    dispatch_value(
        "refresh_all_cpa_oauth",
        json!({ "id": id, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn list_cpa_error_logs(id: String) -> Result<Value, String> {
    dispatch_value("list_cpa_error_logs", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn list_cpa_plugins(id: String) -> Result<Value, String> {
    dispatch_value("list_cpa_plugins", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn list_cpa_plugin_store(id: String) -> Result<Value, String> {
    dispatch_value("list_cpa_plugin_store", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn install_cpa_plugin(
    id: String,
    plugin_id: String,
    source_id: String,
    version: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "install_cpa_plugin",
        json!({ "id": id, "pluginId": plugin_id, "sourceId": source_id, "version": version, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn check_cpa_version(id: String) -> Result<Value, String> {
    dispatch_value("check_cpa_version", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn get_cpa_plugin_config(id: String, plugin_id: String) -> Result<Value, String> {
    dispatch_value(
        "get_cpa_plugin_config",
        json!({ "id": id, "pluginId": plugin_id }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn set_cpa_plugin_enabled(
    id: String,
    plugin_id: String,
    enabled: bool,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "set_cpa_plugin_enabled",
        json!({ "id": id, "pluginId": plugin_id, "enabled": enabled, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn update_cpa_plugin_config(
    id: String,
    plugin_id: String,
    content: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "update_cpa_plugin_config",
        json!({ "id": id, "pluginId": plugin_id, "content": content, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn remove_cpa_plugin(
    id: String,
    plugin_id: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "remove_cpa_plugin",
        json!({ "id": id, "pluginId": plugin_id, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn list_cpa_login_providers(id: String) -> Result<Value, String> {
    dispatch_value("list_cpa_login_providers", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn start_cpa_login(id: String, provider: String) -> Result<Value, String> {
    dispatch_value("start_cpa_login", json!({ "id": id, "provider": provider })).await
}
#[tauri::command]
pub(crate) async fn get_cpa_login_status(
    id: String,
    state: String,
    api_version: String,
) -> Result<Value, String> {
    dispatch_value(
        "get_cpa_login_status",
        json!({ "id": id, "state": state, "apiVersion": api_version }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn submit_cpa_login_callback(
    id: String,
    provider: String,
    state: String,
    api_version: String,
    input: String,
) -> Result<Value, String> {
    dispatch_value(
        "submit_cpa_login_callback",
        json!({ "id": id, "provider": provider, "state": state, "apiVersion": api_version, "input": input }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn cancel_cpa_login(id: String, state: String) -> Result<Value, String> {
    dispatch_value("cancel_cpa_login", json!({ "id": id, "state": state })).await
}
#[tauri::command]
pub(crate) async fn get_cpa_account_quota(
    id: String,
    auth_index: String,
    plugin_id: String,
    api_version: String,
) -> Result<Value, String> {
    dispatch_value(
        "get_cpa_account_quota",
        json!({ "id": id, "authIndex": auth_index, "pluginId": plugin_id, "apiVersion": api_version }),
    )
    .await
}
/// 授权地址由核心校验后交给系统浏览器；浏览器版直接在新标签页打开同一地址。
#[tauri::command]
pub(crate) async fn open_cpa_login_url(id: String, url: String) -> Result<(), String> {
    let target: String = super::dispatch("cpa_login_url", json!({ "id": id, "url": url })).await?;
    balancehub_core::host::open_url(&target).map_err(|_| "无法打开系统默认浏览器".to_string())
}
#[tauri::command]
pub(crate) async fn get_cpa_model_rules(id: String) -> Result<Value, String> {
    dispatch_value("get_cpa_model_rules", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn get_cpa_model_definitions(
    id: String,
    channel: String,
) -> Result<Value, String> {
    dispatch_value(
        "get_cpa_model_definitions",
        json!({ "id": id, "channel": channel }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn update_cpa_model_rules(
    id: String,
    kind: CpaModelRuleKind,
    content: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "update_cpa_model_rules",
        json!({ "id": id, "kind": kind, "content": content, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn get_cpa_gateway_settings(id: String) -> Result<Value, String> {
    dispatch_value("get_cpa_gateway_settings", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn update_cpa_gateway_settings(
    id: String,
    update: CpaGatewaySettingsUpdate,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "update_cpa_gateway_settings",
        json!({ "id": id, "update": update, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn list_cpa_client_keys(id: String) -> Result<Value, String> {
    dispatch_value("list_cpa_client_keys", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn add_cpa_client_key(
    id: String,
    input: CpaClientKeyAdd,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "add_cpa_client_key",
        json!({ "id": id, "input": input, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn remove_cpa_client_key(
    id: String,
    value: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "remove_cpa_client_key",
        json!({ "id": id, "value": value, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn read_cpa_request_logs(
    id: String,
    limit: u32,
    cursor: String,
) -> Result<Value, String> {
    dispatch_value(
        "read_cpa_request_logs",
        json!({ "id": id, "limit": limit, "cursor": cursor }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn read_cpa_api_key_usage(id: String) -> Result<Value, String> {
    dispatch_value("read_cpa_api_key_usage", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn read_cpa_request_log(id: String, request_id: String) -> Result<Value, String> {
    dispatch_value(
        "read_cpa_request_log",
        json!({ "id": id, "requestId": request_id }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn clear_cpa_logs(id: String, revision: u64) -> Result<Value, String> {
    dispatch_value("clear_cpa_logs", json!({ "id": id, "revision": revision })).await
}
#[tauri::command]
pub(crate) async fn read_cpa_gateway_config(id: String) -> Result<Value, String> {
    dispatch_value("read_cpa_gateway_config", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn update_cpa_gateway_config(
    id: String,
    content: String,
    etag: String,
    api_version: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "update_cpa_gateway_config",
        json!({ "id": id, "content": content, "etag": etag, "apiVersion": api_version, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn get_cpa_log_settings(id: String) -> Result<Value, String> {
    dispatch_value("get_cpa_log_settings", json!({ "id": id })).await
}
#[tauri::command]
pub(crate) async fn set_cpa_log_settings(
    id: String,
    enabled: bool,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "set_cpa_log_settings",
        json!({ "id": id, "enabled": enabled, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn read_cpa_error_log(id: String, name: String) -> Result<Value, String> {
    dispatch_value("read_cpa_error_log", json!({ "id": id, "name": name })).await
}
#[tauri::command]
pub(crate) async fn delete_cpa_oauth(
    id: String,
    name: String,
    auth_index: String,
    revision: u64,
) -> Result<Value, String> {
    dispatch_value(
        "delete_cpa_oauth",
        json!({ "id": id, "name": name, "authIndex": auth_index, "revision": revision }),
    )
    .await
}
#[tauri::command]
pub(crate) async fn view_cpa_oauth_file(id: String, name: String) -> Result<Value, String> {
    dispatch_value("view_cpa_oauth_file", json!({ "id": id, "name": name })).await
}
