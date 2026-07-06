use crate::models::{normalize_api_key, ProviderApiKeyOption};
use reqwest::{Client, Method};
use serde_json::{json, Value};

use super::newapi_http::{build_url, build_user_request, UserCredential};
use super::newapi_response::{
    extract_bool_field, extract_f64_field, extract_i64_field, extract_string_field,
    extract_token_items, parse_success_data, send_text,
};
use super::newapi_site::{convert_quota_value, fetch_site_metadata, SiteMetadata};

pub(crate) async fn fetch_api_key_options(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    let url = build_url(base_url, "/api/token/?p=0&page_size=100")?;
    let request = build_user_request(
        client,
        Method::GET,
        url,
        base_url,
        api_user,
        credential.clone(),
        is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "读取 API 密钥列表").await?;
    let data = parse_success_data(&status, body, "API 密钥列表")?;
    let tokens = extract_token_items(&data);
    let site = fetch_site_metadata(client, base_url, is_anyrouter)
        .await
        .unwrap_or_default();

    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut options = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let name = extract_token_name(token).unwrap_or_else(|| format!("API 密钥 #{}", index + 1));
        let token_id = extract_token_id(token).unwrap_or_default();
        let status = extract_token_status(token).unwrap_or_default();
        if let Some(key) = extract_full_key_from_token(token) {
            options.push(api_key_option_from_token(
                token, name, key, token_id, status, &site,
            ));
            continue;
        }

        if token_id.is_empty() {
            continue;
        }

        if let Ok(key) = reveal_api_key(
            client,
            base_url,
            api_user,
            credential.clone(),
            is_anyrouter,
            &token_id,
        )
        .await
        {
            options.push(api_key_option_from_token(
                token, name, key, token_id, status, &site,
            ));
        }
    }

    Ok(options)
}

pub(crate) async fn create_api_key(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
    name: &str,
) -> Result<ProviderApiKeyOption, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("请填写 API 密钥名称".to_string());
    }
    let url = build_url(base_url, "/api/token/")?;
    let payload = json!({
        "name": name,
        "remain_quota": 500000000000i64,
        "unlimited_quota": true,
        "expired_time": -1,
    });
    let request = build_user_request(
        client,
        Method::POST,
        url,
        base_url,
        api_user,
        credential.clone(),
        is_anyrouter,
    )
    .await?
    .json(&payload);
    let (status, body) = send_text(request, "创建 API 密钥").await?;
    let data = parse_success_data(&status, body, "创建 API 密钥")?;
    let site = fetch_site_metadata(client, base_url, is_anyrouter)
        .await
        .unwrap_or_default();

    if let Some(key) = extract_string_field(&data, &["key", "Key"]) {
        return Ok(api_key_option_from_token(
            &data,
            name.to_string(),
            normalize_api_key(&key),
            extract_token_id(&data).unwrap_or_default(),
            extract_token_status(&data).unwrap_or_default(),
            &site,
        ));
    }

    if let Some(token_id) = extract_token_id(&data) {
        let key = reveal_api_key(
            client,
            base_url,
            api_user,
            credential,
            is_anyrouter,
            &token_id,
        )
        .await?;
        return Ok(api_key_option_from_token(
            &data,
            name.to_string(),
            key,
            token_id,
            extract_token_status(&data).unwrap_or_default(),
            &site,
        ));
    }

    let options =
        fetch_api_key_options(client, base_url, api_user, credential, is_anyrouter).await?;
    options
        .into_iter()
        .next()
        .ok_or_else(|| "创建后没有找到可用 API 密钥".to_string())
}

pub(crate) async fn delete_api_key(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
    token_id: &str,
) -> Result<(), String> {
    let url = build_url(base_url, &format!("/api/token/{}", token_id.trim()))?;
    let request = build_user_request(
        client,
        Method::DELETE,
        url,
        base_url,
        api_user,
        credential,
        is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "删除 API 密钥").await?;
    parse_success_data(&status, body, "删除 API 密钥")?;
    Ok(())
}

async fn reveal_api_key(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
    token_id: &str,
) -> Result<String, String> {
    let reveal_url = build_url(base_url, &format!("/api/token/{token_id}/key"))?;
    let reveal_request = build_user_request(
        client,
        Method::POST,
        reveal_url,
        base_url,
        api_user,
        credential,
        is_anyrouter,
    )
    .await?
    .body("");
    let (status, body) = send_text(reveal_request, "读取完整 API 密钥").await?;
    let reveal_data = parse_success_data(&status, body, "完整 API 密钥")?;
    extract_string_field(&reveal_data, &["key", "Key"])
        .filter(|key| is_full_api_key(key))
        .map(|key| normalize_api_key(&key))
        .ok_or_else(|| "接口没有返回完整 API 密钥".to_string())
}

fn extract_full_key_from_token(token: &Value) -> Option<String> {
    extract_string_field(token, &["key", "Key"])
        .filter(|key| is_full_api_key(key))
        .map(|key| normalize_api_key(&key))
}

fn api_key_option_from_token(
    token: &Value,
    name: String,
    key: String,
    token_id: String,
    status: String,
    site: &SiteMetadata,
) -> ProviderApiKeyOption {
    ProviderApiKeyOption {
        name,
        key,
        token_id,
        status,
        used_quota: converted_quota_field(token, &["used_quota", "usedQuota"], site),
        remain_quota: converted_quota_field(token, &["remain_quota", "remainQuota"], site),
        unlimited_quota: extract_bool_field(token, &["unlimited_quota", "unlimitedQuota"])
            .unwrap_or(false),
        group: extract_string_field(token, &["group", "Group"]).unwrap_or_default(),
        model_limits_enabled: extract_bool_field(
            token,
            &["model_limits_enabled", "modelLimitsEnabled"],
        )
        .unwrap_or(false),
        model_limits: extract_string_list_field(token, &["model_limits", "modelLimits"]),
        allow_ips: extract_string_list_field(token, &["allow_ips", "allowIps", "allowed_ips"]),
        created_time: extract_i64_field(token, &["created_time", "createdTime", "created_at"]),
        accessed_time: extract_i64_field(token, &["accessed_time", "accessedTime"]),
        expired_time: extract_i64_field(token, &["expired_time", "expiredTime"]),
    }
}

fn converted_quota_field(token: &Value, field_names: &[&str], site: &SiteMetadata) -> f64 {
    let value = extract_i64_field(token, field_names)
        .or_else(|| extract_f64_field(token, field_names).map(|value| value as i64))
        .unwrap_or(0);
    convert_quota_value(value, site).0
}

fn extract_string_list_field(token: &Value, field_names: &[&str]) -> Vec<String> {
    field_names
        .iter()
        .find_map(|field_name| token.get(*field_name))
        .map(|value| match value {
            Value::Array(items) => items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(ToString::to_string)
                })
                .collect(),
            Value::String(text) => text
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn extract_token_name(token: &Value) -> Option<String> {
    extract_string_field(token, &["name", "Name", "token_name", "tokenName"])
}

fn extract_token_status(token: &Value) -> Option<String> {
    if let Some(status) = extract_string_field(token, &["status", "Status"]) {
        return Some(status);
    }

    token
        .get("status")
        .or_else(|| token.get("Status"))
        .and_then(|value| {
            value
                .as_i64()
                .map(|number| number.to_string())
                .or_else(|| value.as_u64().map(|number| number.to_string()))
        })
}

fn extract_token_id(token: &Value) -> Option<String> {
    if let Some(id) = token
        .get("id")
        .or_else(|| token.get("Id"))
        .or_else(|| token.get("ID"))
    {
        if let Some(id) = id.as_i64() {
            return Some(id.to_string());
        }
        if let Some(id) = id.as_str() {
            return Some(id.to_string());
        }
    }
    None
}

fn is_full_api_key(key: &str) -> bool {
    let trimmed = key.trim();
    !trimmed.is_empty() && !trimmed.contains('*')
}
