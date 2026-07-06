pub use super::newapi_account::change_user_password;
use super::newapi_checkin::probe_check_in_capability;
pub use super::newapi_checkin::{check_in_provider, fetch_check_in_records};
pub use super::newapi_credentials::{complete_credentials, create_access_token};
pub use super::newapi_http::{build_client, is_anyrouter_base_url, provider_is_anyrouter};
use super::newapi_http::{
    build_url, build_user_request, normalize_base_url, provider_user_management_context,
};
use super::newapi_keys::{create_api_key, delete_api_key, fetch_api_key_options};
pub use super::newapi_logs::fetch_request_logs;
pub use super::newapi_quota::{refresh_provider, test_connection};
use super::newapi_response::{extract_string_field, parse_success_data, send_text};
use super::newapi_site::fetch_site_metadata;
pub use super::newapi_site::SiteMetadata;
pub use super::newapi_usage::fetch_usage_summary;
use crate::models::{AuthMode, Provider, ProviderApiKeyOption, ProviderCapabilities};
use reqwest::{Client, Method};

pub async fn discover_site_metadata(
    client: &Client,
    base_url: &str,
) -> Result<SiteMetadata, String> {
    let base_url = normalize_base_url(base_url);
    if base_url.is_empty() {
        return Err("请先填写中转站地址".to_string());
    }
    fetch_site_metadata(client, &base_url, is_anyrouter_base_url(&base_url)).await
}

pub async fn list_api_keys(
    client: &Client,
    provider: &Provider,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    fetch_api_key_options(client, &base_url, &api_user, credential, is_anyrouter).await
}

pub async fn create_managed_api_key(
    client: &Client,
    provider: &Provider,
    name: &str,
) -> Result<ProviderApiKeyOption, String> {
    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    create_api_key(client, &base_url, &api_user, credential, is_anyrouter, name).await
}

pub async fn delete_managed_api_key(
    client: &Client,
    provider: &Provider,
    token_id: &str,
) -> Result<(), String> {
    if token_id.trim().is_empty() {
        return Err("缺少 API 密钥 ID".to_string());
    }
    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    delete_api_key(
        client,
        &base_url,
        &api_user,
        credential,
        is_anyrouter,
        token_id,
    )
    .await
}

pub async fn probe_capabilities(
    client: &Client,
    provider: &Provider,
) -> (ProviderCapabilities, String, Option<String>) {
    let mut capabilities = ProviderCapabilities::default();
    let mut invite_link = String::new();
    let mut errors = Vec::new();
    let base_url = normalize_base_url(&provider.identity.base_url);

    if base_url.is_empty() {
        return (
            capabilities,
            invite_link,
            Some("缺少中转站地址，无法探测站点能力".to_string()),
        );
    }

    let is_anyrouter = provider_is_anyrouter(provider);
    if is_anyrouter {
        capabilities.check_in_known = true;
        capabilities.check_in_supported = !provider.auth.session_cookie.trim().is_empty();
        if capabilities.check_in_supported {
            capabilities.check_in_auth_modes.push(AuthMode::Session);
        }
    } else {
        match probe_check_in_capability(client, provider, &base_url).await {
            Ok(modes) => {
                capabilities.check_in_known = true;
                capabilities.check_in_supported = !modes.is_empty();
                capabilities.check_in_auth_modes = modes;
            }
            Err(message) => errors.push(format!("签到能力: {message}")),
        }
    }

    match list_api_keys(client, provider).await {
        Ok(_) => {
            capabilities.api_key_management_known = true;
            capabilities.api_key_management_supported = true;
        }
        Err(message) => {
            capabilities.api_key_management_known = true;
            capabilities.api_key_management_supported = false;
            errors.push(format!("密钥管理: {message}"));
        }
    }

    match fetch_invite_link(client, provider).await {
        Ok(link) => {
            capabilities.invitation_known = true;
            capabilities.invitation_supported = true;
            invite_link = link;
        }
        Err(message) => {
            capabilities.invitation_known = true;
            capabilities.invitation_supported = false;
            errors.push(format!("邀请链接: {message}"));
        }
    }

    let error = if errors.is_empty() {
        None
    } else {
        Some(errors.join("；"))
    };

    (capabilities, invite_link, error)
}

pub async fn fetch_invite_link(client: &Client, provider: &Provider) -> Result<String, String> {
    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    let url = build_url(&base_url, "/api/user/aff")?;
    let request = build_user_request(
        client,
        Method::GET,
        url,
        &base_url,
        &api_user,
        credential,
        is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "读取邀请链接").await?;
    let data = parse_success_data(&status, body, "邀请链接")?;
    let code = data
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            extract_string_field(
                &data,
                &["aff_code", "affCode", "code", "invite_code", "inviteCode"],
            )
        })
        .ok_or_else(|| "接口没有返回邀请码".to_string())?;
    Ok(format!(
        "{}/register?aff={}",
        base_url.trim_end_matches('/'),
        code
    ))
}
