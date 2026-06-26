use crate::models::{normalize_api_key, ProviderApiKeyOption};
use reqwest::{Client, Method};
use serde_json::{json, Value};

use super::newapi_http::{build_url, build_user_request, UserCredential};
use super::newapi_response::{
    extract_string_field, extract_token_items, parse_success_data, send_text,
};

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

    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut options = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let name = extract_token_name(token).unwrap_or_else(|| format!("API 密钥 #{}", index + 1));
        let token_id = extract_token_id(token).unwrap_or_default();
        let status = extract_token_status(token).unwrap_or_default();
        if let Some(key) = extract_full_key_from_token(token) {
            options.push(ProviderApiKeyOption {
                name,
                key,
                token_id,
                status,
            });
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
            options.push(ProviderApiKeyOption {
                name,
                key,
                token_id,
                status,
            });
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

    if let Some(key) = extract_string_field(&data, &["key", "Key"]) {
        return Ok(ProviderApiKeyOption {
            name: name.to_string(),
            key: normalize_api_key(&key),
            token_id: extract_token_id(&data).unwrap_or_default(),
            status: extract_token_status(&data).unwrap_or_default(),
        });
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
        return Ok(ProviderApiKeyOption {
            name: name.to_string(),
            key,
            token_id,
            status: extract_token_status(&data).unwrap_or_default(),
        });
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
