//! OAuth 账号与 CPA 网关命令。
//!
//! 账号授权、授权文件交换和 CPA 管理都只依赖 HTTP 与核心服务，桌面与服务端
//! 共用同一份实现。打开授权页面属于宿主能力：核心只给出经过校验的地址，
//! 由桌面交给系统浏览器、浏览器版交给当前标签页。

use serde_json::Value;

use super::{arg, blocking, optional, to_value};
use crate::{
    contracts::{provider_views, ProviderView},
    models::*,
    services::{
        oauth_accounts::{imports, login},
        provider_service::ProviderService,
    },
};

pub const COMMANDS: &[&str] = &[
    "start_oauth_login",
    "list_oauth_logins",
    "cancel_oauth_login",
    "submit_oauth_callback",
    "retry_oauth_login_save",
    "oauth_login_url",
    "preview_oauth_import",
    "discard_oauth_import",
    "apply_oauth_import",
    "get_oauth_account",
    "set_oauth_renewal",
    "update_oauth_account",
    "get_oauth_credential",
    "replace_oauth_credential",
    "export_oauth_account_text",
    "preview_cpa_oauth_import",
    "send_oauth_to_cpa",
    "set_cpa_oauth_disabled",
    "update_cpa_oauth_fields",
    "refresh_cpa_oauth",
    "reset_cpa_oauth_cooldown",
    "refresh_all_cpa_oauth",
    "list_cpa_error_logs",
    "list_cpa_plugins",
    "list_cpa_plugin_store",
    "install_cpa_plugin",
    "check_cpa_version",
    "get_cpa_plugin_config",
    "set_cpa_plugin_enabled",
    "update_cpa_plugin_config",
    "remove_cpa_plugin",
    "list_cpa_login_providers",
    "start_cpa_login",
    "get_cpa_login_status",
    "submit_cpa_login_callback",
    "cancel_cpa_login",
    "get_cpa_account_quota",
    "cpa_login_url",
    "get_cpa_model_rules",
    "get_cpa_model_definitions",
    "update_cpa_model_rules",
    "get_cpa_gateway_settings",
    "update_cpa_gateway_settings",
    "list_cpa_client_keys",
    "add_cpa_client_key",
    "remove_cpa_client_key",
    "read_cpa_request_logs",
    "read_cpa_api_key_usage",
    "read_cpa_request_log",
    "clear_cpa_logs",
    "read_cpa_gateway_config",
    "update_cpa_gateway_config",
    "get_cpa_log_settings",
    "set_cpa_log_settings",
    "read_cpa_error_log",
    "delete_cpa_oauth",
    "view_cpa_oauth_file",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    let service = ProviderService::new();
    match command {
        // ── OAuth 授权 ────────────────────────────────────────────────
        "start_oauth_login" => {
            let vendor: OAuthVendor = arg(args, "vendor")?;
            let provider_id: Option<String> = optional(args, "providerId")?;
            to_value(login::start(vendor, provider_id).await?)
        }
        "list_oauth_logins" => to_value(login::list()),
        "cancel_oauth_login" => {
            let id: String = arg(args, "id")?;
            login::cancel(&id)?;
            Ok(Value::Null)
        }
        "submit_oauth_callback" => {
            let id: String = arg(args, "id")?;
            let value: String = arg(args, "value")?;
            login::submit(&id, &value)?;
            Ok(Value::Null)
        }
        "retry_oauth_login_save" => {
            let id: String = arg(args, "id")?;
            login::retry_save(id)?;
            Ok(Value::Null)
        }
        "oauth_login_url" => {
            let id: String = arg(args, "id")?;
            to_value(login::authorization_url(&id)?)
        }
        "preview_oauth_import" => {
            let files: Vec<OAuthImportFile> = arg(args, "files")?;
            let paths: Vec<String> = arg(args, "paths")?;
            let vendor: Option<OAuthVendor> = optional(args, "vendor")?;
            blocking(move || {
                if crate::host::is_headless() && !paths.is_empty() {
                    return Err("浏览器版请上传授权文件内容，而不是服务器路径".into());
                }
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
            .and_then(to_value)
        }
        "discard_oauth_import" => {
            let token: String = arg(args, "token")?;
            imports::discard(&token);
            Ok(Value::Null)
        }
        "apply_oauth_import" => {
            let request: OAuthImportRequest = arg(args, "request")?;
            let providers = service.import_oauth_accounts(request).await?;
            to_value(provider_views(providers))
        }
        "get_oauth_account" => {
            let id: String = arg(args, "id")?;
            to_value(service.oauth_details(id).await?)
        }
        "set_oauth_renewal" => {
            let id: String = arg(args, "id")?;
            let enabled: bool = arg(args, "enabled")?;
            to_value(ProviderView::from(
                service.set_oauth_renewal(id, enabled).await?,
            ))
        }
        "update_oauth_account" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let remark: String = arg(args, "remark")?;
            let refresh_interval: u64 = arg(args, "refreshInterval")?;
            to_value(ProviderView::from(
                service
                    .update_oauth_metadata(id, name, remark, refresh_interval)
                    .await?,
            ))
        }
        "get_oauth_credential" => {
            let id: String = arg(args, "id")?;
            to_value(service.oauth_credential_source(id).await?)
        }
        "replace_oauth_credential" => {
            let id: String = arg(args, "id")?;
            let content: String = arg(args, "content")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(ProviderView::from(
                service
                    .replace_oauth_credential(id, content, revision)
                    .await?,
            ))
        }
        "export_oauth_account_text" => {
            let id: String = arg(args, "id")?;
            let format: OAuthExportFormat = arg(args, "format")?;
            let pause_renewal: bool = arg(args, "pauseRenewal")?;
            to_value(
                service
                    .export_oauth_content(id, format, pause_renewal)
                    .await?,
            )
        }

        // ── CPA 网关 ──────────────────────────────────────────────────
        "preview_cpa_oauth_import" => {
            let id: String = arg(args, "id")?;
            let names: Vec<String> = arg(args, "names")?;
            to_value(service.import_cpa_preview(id, names).await?)
        }
        "send_oauth_to_cpa" => {
            let id: String = arg(args, "id")?;
            let gateway_id: String = arg(args, "gatewayId")?;
            to_value(service.transfer_oauth_to_cpa(id, gateway_id).await?)
        }
        "set_cpa_oauth_disabled" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let auth_index: String = arg(args, "authIndex")?;
            let disabled: bool = arg(args, "disabled")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .set_cpa_account_disabled(id, name, auth_index, disabled, revision)
                .await?;
            Ok(Value::Null)
        }
        "update_cpa_oauth_fields" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let auth_index: String = arg(args, "authIndex")?;
            let fields: CpaCredentialFields = arg(args, "fields")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .set_cpa_account_fields(id, name, auth_index, fields, revision)
                .await?;
            Ok(Value::Null)
        }
        "refresh_cpa_oauth" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let auth_index: String = arg(args, "authIndex")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .refresh_cpa_account(id, name, auth_index, revision)
                .await?;
            Ok(Value::Null)
        }
        "reset_cpa_oauth_cooldown" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let auth_index: String = arg(args, "authIndex")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .reset_cpa_account_cooldown(id, name, auth_index, revision)
                .await?;
            Ok(Value::Null)
        }
        "refresh_all_cpa_oauth" => {
            let id: String = arg(args, "id")?;
            let revision: u64 = arg(args, "revision")?;
            service.refresh_all_cpa_accounts(id, revision).await?;
            Ok(Value::Null)
        }
        "list_cpa_error_logs" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_error_logs(id).await?)
        }
        "list_cpa_plugins" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_plugins(id).await?)
        }
        "list_cpa_plugin_store" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_plugin_store(id).await?)
        }
        "install_cpa_plugin" => {
            let id: String = arg(args, "id")?;
            let plugin_id: String = arg(args, "pluginId")?;
            let source_id: String = arg(args, "sourceId")?;
            let version: String = arg(args, "version")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(
                service
                    .install_cpa_plugin(id, plugin_id, source_id, version, revision)
                    .await?,
            )
        }
        "check_cpa_version" => {
            let id: String = arg(args, "id")?;
            to_value(service.check_cpa_version(id).await?)
        }
        "get_cpa_plugin_config" => {
            let id: String = arg(args, "id")?;
            let plugin_id: String = arg(args, "pluginId")?;
            to_value(service.cpa_plugin_config(id, plugin_id).await?)
        }
        "set_cpa_plugin_enabled" => {
            let id: String = arg(args, "id")?;
            let plugin_id: String = arg(args, "pluginId")?;
            let enabled: bool = arg(args, "enabled")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .set_cpa_plugin_enabled(id, plugin_id, enabled, revision)
                .await?;
            Ok(Value::Null)
        }
        "update_cpa_plugin_config" => {
            let id: String = arg(args, "id")?;
            let plugin_id: String = arg(args, "pluginId")?;
            let content: String = arg(args, "content")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(
                service
                    .update_cpa_plugin_config(id, plugin_id, content, revision)
                    .await?,
            )
        }
        "remove_cpa_plugin" => {
            let id: String = arg(args, "id")?;
            let plugin_id: String = arg(args, "pluginId")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(service.remove_cpa_plugin(id, plugin_id, revision).await?)
        }
        "list_cpa_login_providers" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_login_providers(id).await?)
        }
        "start_cpa_login" => {
            let id: String = arg(args, "id")?;
            let provider: String = arg(args, "provider")?;
            to_value(service.start_cpa_login(id, provider).await?)
        }
        "get_cpa_login_status" => {
            let id: String = arg(args, "id")?;
            let state: String = arg(args, "state")?;
            let api_version: String = arg(args, "apiVersion")?;
            to_value(service.cpa_login_status(id, state, api_version).await?)
        }
        "submit_cpa_login_callback" => {
            let id: String = arg(args, "id")?;
            let provider: String = arg(args, "provider")?;
            let state: String = arg(args, "state")?;
            let api_version: String = arg(args, "apiVersion")?;
            let input: String = arg(args, "input")?;
            service
                .submit_cpa_login_callback(id, provider, state, api_version, input)
                .await?;
            Ok(Value::Null)
        }
        "cancel_cpa_login" => {
            let id: String = arg(args, "id")?;
            let state: String = arg(args, "state")?;
            service.cancel_cpa_login(id, state).await?;
            Ok(Value::Null)
        }
        "get_cpa_account_quota" => {
            let id: String = arg(args, "id")?;
            let auth_index: String = arg(args, "authIndex")?;
            let plugin_id: String = arg(args, "pluginId")?;
            let api_version: String = arg(args, "apiVersion")?;
            to_value(
                service
                    .cpa_account_quota(id, auth_index, plugin_id, api_version)
                    .await?,
            )
        }
        "cpa_login_url" => {
            let id: String = arg(args, "id")?;
            let url: String = arg(args, "url")?;
            blocking(move || {
                let data = ProviderService::new().load_app_data()?;
                let provider = data
                    .providers
                    .iter()
                    .find(|provider| provider.identity.id == id)
                    .ok_or("CPA 网关已删除")?;
                crate::services::oauth_accounts::cpa_login_target(provider, &url)
            })
            .await
            .and_then(to_value)
        }
        "get_cpa_model_rules" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_model_rules(id).await?)
        }
        "get_cpa_model_definitions" => {
            let id: String = arg(args, "id")?;
            let channel: String = arg(args, "channel")?;
            to_value(service.cpa_model_definitions(id, channel).await?)
        }
        "update_cpa_model_rules" => {
            let id: String = arg(args, "id")?;
            let kind: CpaModelRuleKind = arg(args, "kind")?;
            let content: String = arg(args, "content")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(
                service
                    .update_cpa_model_rules(id, kind, content, revision)
                    .await?,
            )
        }
        "get_cpa_gateway_settings" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_gateway_settings(id).await?.0)
        }
        "update_cpa_gateway_settings" => {
            let id: String = arg(args, "id")?;
            let update: CpaGatewaySettingsUpdate = arg(args, "update")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .update_cpa_gateway_settings(id, update, revision)
                .await?;
            Ok(Value::Null)
        }
        "list_cpa_client_keys" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_client_keys(id).await?)
        }
        "add_cpa_client_key" => {
            let id: String = arg(args, "id")?;
            let input: CpaClientKeyAdd = arg(args, "input")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(service.add_cpa_client_key(id, input, revision).await?)
        }
        "remove_cpa_client_key" => {
            let id: String = arg(args, "id")?;
            let value: String = arg(args, "value")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(service.remove_cpa_client_key(id, value, revision).await?)
        }
        "read_cpa_request_logs" => {
            let id: String = arg(args, "id")?;
            let limit: u32 = arg(args, "limit")?;
            let cursor: String = arg(args, "cursor")?;
            to_value(service.cpa_request_logs(id, limit, cursor).await?)
        }
        "read_cpa_api_key_usage" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_api_key_usage(id).await?)
        }
        "read_cpa_request_log" => {
            let id: String = arg(args, "id")?;
            let request_id: String = arg(args, "requestId")?;
            to_value(service.cpa_request_log(id, request_id).await?)
        }
        "clear_cpa_logs" => {
            let id: String = arg(args, "id")?;
            let revision: u64 = arg(args, "revision")?;
            to_value(service.clear_cpa_logs(id, revision).await?)
        }
        "read_cpa_gateway_config" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_gateway_config(id).await?)
        }
        "update_cpa_gateway_config" => {
            let id: String = arg(args, "id")?;
            let content: String = arg(args, "content")?;
            let etag: String = arg(args, "etag")?;
            let api_version: String = arg(args, "apiVersion")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .update_cpa_gateway_config(id, content, etag, api_version, revision)
                .await?;
            Ok(Value::Null)
        }
        "get_cpa_log_settings" => {
            let id: String = arg(args, "id")?;
            to_value(service.cpa_log_settings(id).await?)
        }
        "set_cpa_log_settings" => {
            let id: String = arg(args, "id")?;
            let enabled: bool = arg(args, "enabled")?;
            let revision: u64 = arg(args, "revision")?;
            service.set_cpa_log_settings(id, enabled, revision).await?;
            Ok(Value::Null)
        }
        "read_cpa_error_log" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            to_value(service.read_cpa_error_log(id, name).await?)
        }
        "delete_cpa_oauth" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let auth_index: String = arg(args, "authIndex")?;
            let revision: u64 = arg(args, "revision")?;
            service
                .delete_cpa_account(id, name, auth_index, revision)
                .await?;
            Ok(Value::Null)
        }
        "view_cpa_oauth_file" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            to_value(service.read_cpa_account_file(id, name).await?)
        }
        _ => Err(format!("未实现的账号命令：{command}")),
    }
}
