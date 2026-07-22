use crate::models::{normalize_api_key, ProviderApiKeyOption};
use reqwest::{Client, Method};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::newapi_http::{build_url, build_user_request, UserCredential};
use super::newapi_response::{
    extract_bool_field, extract_f64_field, extract_i64_field, extract_string_field,
    extract_token_items, parse_success_data, send_text,
};
use super::newapi_site::{convert_quota_value, fetch_site_metadata, SiteMetadata};

const API_KEY_PAGE_SIZE: usize = 100;

pub(crate) async fn fetch_api_key_options(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
) -> Result<Vec<ProviderApiKeyOption>, String> {
    let data = fetch_api_key_page(
        client,
        base_url,
        api_user,
        credential.clone(),
        is_anyrouter,
        1,
    )
    .await?;
    // NewAPI caps this endpoint at 100 items. Enforce the same bound locally
    // even when a compatible deployment ignores the requested page size.
    let tokens = extract_token_items(&data)
        .into_iter()
        .take(API_KEY_PAGE_SIZE)
        .collect::<Vec<_>>();
    let site = fetch_site_metadata(client, base_url, is_anyrouter)
        .await
        .unwrap_or_default();

    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let token_ids = tokens
        .iter()
        .filter_map(extract_token_id)
        .collect::<Vec<_>>();
    let batch_keys = reveal_api_keys_batch(
        client,
        base_url,
        api_user,
        credential.clone(),
        is_anyrouter,
        &token_ids,
    )
    .await
    .unwrap_or_default();

    let mut options = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let name = extract_token_name(token).unwrap_or_else(|| format!("API 密钥 #{}", index + 1));
        let token_id = extract_token_id(token).unwrap_or_default();
        let status = extract_token_status(token).unwrap_or_default();
        let mut full_key =
            extract_full_key_from_token(token).or_else(|| batch_keys.get(&token_id).cloned());
        if full_key.is_none() && !token_id.is_empty() {
            full_key = reveal_api_key(
                client,
                base_url,
                api_user,
                credential.clone(),
                is_anyrouter,
                &token_id,
            )
            .await
            .ok();
        }
        options.push(api_key_option_from_token(
            token, name, full_key, token_id, status, &site,
        ));
    }

    Ok(options)
}

pub(crate) async fn probe_api_key_management(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
) -> Result<(), String> {
    fetch_api_key_page(client, base_url, api_user, credential, is_anyrouter, 1)
        .await
        .map(|_| ())
}

async fn fetch_api_key_page(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
    page: usize,
) -> Result<Value, String> {
    let mut url = build_url(base_url, "/api/token/")?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("p", &page.to_string());
        pairs.append_pair("page_size", &API_KEY_PAGE_SIZE.to_string());
    }
    let request = build_user_request(
        client,
        Method::GET,
        url,
        base_url,
        api_user,
        credential,
        is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "读取 API 密钥列表").await?;
    parse_success_data(&status, body, "API 密钥列表")
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
            Some(normalize_api_key(&key)),
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
            Some(key),
            token_id,
            extract_token_status(&data).unwrap_or_default(),
            &site,
        ));
    }

    let options =
        fetch_api_key_options(client, base_url, api_user, credential, is_anyrouter).await?;
    options
        .iter()
        .find(|option| option.name == name && option.key_available)
        .cloned()
        .or_else(|| options.into_iter().find(|option| option.key_available))
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

async fn reveal_api_keys_batch(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
    token_ids: &[String],
) -> Result<HashMap<String, String>, String> {
    let ids = token_ids
        .iter()
        .filter_map(|id| id.parse::<i64>().ok())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let url = build_url(base_url, "/api/token/batch/keys")?;
    let request = build_user_request(
        client,
        Method::POST,
        url,
        base_url,
        api_user,
        credential,
        is_anyrouter,
    )
    .await?
    .json(&json!({ "ids": ids }));
    let (status, body) = send_text(request, "批量读取完整 API 密钥").await?;
    let data = parse_success_data(&status, body, "完整 API 密钥")?;
    let keys = data.get("keys").unwrap_or(&data);
    let Some(keys) = keys.as_object() else {
        return Err("接口没有返回 API 密钥映射".to_string());
    };

    Ok(keys
        .iter()
        .filter_map(|(id, value)| {
            value
                .as_str()
                .map(normalize_api_key)
                .filter(|key| is_full_api_key(key))
                .map(|key| (id.clone(), key))
        })
        .collect())
}

fn extract_full_key_from_token(token: &Value) -> Option<String> {
    extract_string_field(token, &["key", "Key"])
        .filter(|key| is_full_api_key(key))
        .map(|key| normalize_api_key(&key))
}

fn api_key_option_from_token(
    token: &Value,
    name: String,
    key: Option<String>,
    token_id: String,
    status: String,
    site: &SiteMetadata,
) -> ProviderApiKeyOption {
    let key = key
        .map(|value| normalize_api_key(&value))
        .filter(|value| is_full_api_key(value))
        .unwrap_or_default();
    let masked_key = extract_string_field(token, &["key", "Key"])
        .filter(|value| value.contains('*'))
        .unwrap_or_else(|| ProviderApiKeyOption::current(&key).masked_key);
    let used_quota_raw = raw_quota_field(token, &["used_quota", "usedQuota"]);
    let remain_quota_raw = raw_quota_field(token, &["remain_quota", "remainQuota"]);

    ProviderApiKeyOption {
        name,
        key_available: !key.is_empty(),
        key,
        masked_key,
        token_id,
        user_id: extract_string_field(token, &["user_id", "userId"]).unwrap_or_default(),
        status,
        used_quota: convert_quota_value(used_quota_raw, site).0,
        remain_quota: convert_quota_value(remain_quota_raw, site).0,
        used_quota_raw,
        remain_quota_raw,
        unlimited_quota: extract_bool_field(token, &["unlimited_quota", "unlimitedQuota"])
            .unwrap_or(false),
        group: extract_string_field(token, &["group", "Group"]).unwrap_or_default(),
        cross_group_retry: extract_bool_field(token, &["cross_group_retry", "crossGroupRetry"])
            .unwrap_or(false),
        model_limits_enabled: extract_bool_field(
            token,
            &["model_limits_enabled", "modelLimitsEnabled"],
        )
        .unwrap_or(false),
        model_limits: extract_string_list_field(token, &["model_limits", "modelLimits"]),
        allow_ips: extract_string_list_field(token, &["allow_ips", "allowIps", "allowed_ips"]),
        quota_display_type: site.quota_display_type.clone(),
        currency_symbol: site.currency_symbol.clone(),
        created_time: extract_i64_field(token, &["created_time", "createdTime", "created_at"]),
        accessed_time: extract_i64_field(token, &["accessed_time", "accessedTime"]),
        expired_time: extract_i64_field(token, &["expired_time", "expiredTime"]),
    }
    .normalize()
}

fn raw_quota_field(token: &Value, field_names: &[&str]) -> i64 {
    extract_i64_field(token, field_names)
        .or_else(|| extract_f64_field(token, field_names).map(|value| value as i64))
        .unwrap_or(0)
}

fn extract_string_list_field(token: &Value, field_names: &[&str]) -> Vec<String> {
    field_names
        .iter()
        .find_map(|field_name| token.get(*field_name))
        .map(extract_string_list_value)
        .unwrap_or_default()
}

fn extract_string_list_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)
            })
            .collect(),
        Value::String(text) => {
            let trimmed = text.trim();
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                if parsed.is_array() || parsed.is_object() {
                    return extract_string_list_value(&parsed);
                }
            }
            trimmed
                .split([',', '\n', '\r'])
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect()
        }
        Value::Object(items) => items
            .iter()
            .filter(|(_, enabled)| enabled.as_bool().unwrap_or(true))
            .map(|(name, _)| name.trim())
            .filter(|name| !name.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_metadata_maps_all_supported_newapi_fields() {
        let token = json!({
            "id": 17,
            "user_id": 9,
            "key": "abcd**********wxyz",
            "status": 1,
            "name": "Codex",
            "created_time": 1_700_000_000,
            "accessed_time": 1_700_000_100,
            "expired_time": -1,
            "remain_quota": 500_000,
            "used_quota": 250_000,
            "unlimited_quota": false,
            "model_limits_enabled": true,
            "model_limits": "gpt-5,codex-mini",
            "allow_ips": "127.0.0.1\n10.0.0.1",
            "group": "default",
            "cross_group_retry": true
        });

        let option = api_key_option_from_token(
            &token,
            "Codex".to_string(),
            Some("raw-key-value".to_string()),
            "17".to_string(),
            "1".to_string(),
            &SiteMetadata::default(),
        );

        assert_eq!(option.key, "sk-raw-key-value");
        assert_eq!(option.masked_key, "abcd**********wxyz");
        assert_eq!(option.token_id, "17");
        assert_eq!(option.user_id, "9");
        assert_eq!(option.used_quota_raw, 250_000);
        assert_eq!(option.remain_quota_raw, 500_000);
        assert_eq!(option.group, "default");
        assert!(option.cross_group_retry);
        assert!(option.model_limits_enabled);
        assert_eq!(option.model_limits, vec!["gpt-5", "codex-mini"]);
        assert_eq!(option.allow_ips, vec!["127.0.0.1", "10.0.0.1"]);
        assert_eq!(option.created_time, Some(1_700_000_000));
        assert_eq!(option.accessed_time, Some(1_700_000_100));
        assert_eq!(option.expired_time, Some(-1));
    }

    #[test]
    fn string_list_supports_newapi_map_and_json_array_variants() {
        assert_eq!(
            extract_string_list_value(&json!({"gpt-5": true, "disabled": false})),
            vec!["gpt-5"]
        );
        assert_eq!(
            extract_string_list_value(&Value::String(r#"["gpt-5","claude-sonnet"]"#.to_string())),
            vec!["gpt-5", "claude-sonnet"]
        );
    }
}
