use crate::{
    adapters::transport::ProviderTransport,
    limits,
    models::{Provider, ProviderApiKeyOption, ProviderProtocol},
};
use reqwest::Method;
use serde_json::Value;

use super::{
    auth::request_account_json,
    json::{
        array_items, number_field, string_field, string_list, timestamp_millis, value_has_field,
    },
};

pub(super) async fn fetch_api_keys(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<(Provider, Vec<ProviderApiKeyOption>), String> {
    let (authenticated, data) = request_account_json(
        client,
        provider,
        Method::GET,
        "/keys?page=1&page_size=100&sort_by=created_at&sort_order=desc",
        None,
        "读取 Sub2API API Key 列表",
    )
    .await?;
    let items = array_items(&data);
    let mut options = items
        .into_iter()
        .take(limits::MAX_API_KEYS_PER_PROVIDER)
        .filter_map(|item| api_key_from_value(&item))
        .collect::<Vec<_>>();
    ProviderApiKeyOption::merge_cached_key_material(
        &mut options,
        &provider.auth.api_key_options,
        ProviderProtocol::Sub2Api,
    );
    Ok((authenticated, options))
}

pub(super) fn api_key_from_value(value: &Value) -> Option<ProviderApiKeyOption> {
    let value = value
        .get("api_key")
        .or_else(|| value.get("apiKey"))
        .or_else(|| value.get("token"))
        .filter(|item| item.is_object())
        .unwrap_or(value);
    let key = string_field(value, &["key", "api_key", "value"]).unwrap_or_default();
    let masked_key =
        string_field(value, &["masked_key", "maskedKey", "key_masked"]).unwrap_or_default();
    let token_id = string_field(value, &["id", "token_id", "tokenId"]).unwrap_or_default();
    let name = string_field(value, &["name"]).unwrap_or_else(|| {
        if token_id.is_empty() {
            "API Key".to_string()
        } else {
            format!("API Key #{token_id}")
        }
    });
    let quota_fields = ["quota", "quota_limit", "quotaLimit"];
    let quota = number_field(value, &quota_fields);
    let used = number_field(value, &["quota_used", "quotaUsed"]);
    let unlimited = value_has_field(value, &quota_fields) && quota <= 0.0;
    let group = value
        .get("group")
        .and_then(|group| string_field(group, &["name", "id", "group_id"]))
        .or_else(|| string_field(value, &["group_name", "groupName"]))
        .or_else(|| string_field(value, &["group_id", "groupId"]))
        .unwrap_or_default();
    let mut option = ProviderApiKeyOption::current_for_protocol(&key, ProviderProtocol::Sub2Api);
    option.name = name;
    option.key = key;
    option.key_available = !option.key.trim().is_empty() && !option.key.contains('*');
    option.masked_key = if masked_key.is_empty() {
        mask_key(&option.key)
    } else {
        masked_key
    };
    option.token_id = token_id;
    option.user_id = string_field(value, &["user_id", "userId"]).unwrap_or_default();
    option.status = normalize_key_status(value);
    option.used_quota = used;
    option.remain_quota = if unlimited {
        0.0
    } else {
        (quota - used).max(0.0)
    };
    option.used_quota_raw = used.round() as i64;
    option.remain_quota_raw = option.remain_quota.round() as i64;
    option.unlimited_quota = unlimited;
    option.group = group;
    option.allow_ips = string_list(value, &["ip_whitelist", "ipWhitelist"]);
    option.created_time = timestamp_millis(value, &["created_at", "createdAt"]);
    option.accessed_time = timestamp_millis(value, &["last_used_at", "lastUsedAt"]);
    option.expired_time = timestamp_millis(value, &["expires_at", "expiresAt"]);
    option.model_limits.extend(string_list(
        value,
        &["model_limits", "modelLimits", "allowed_models"],
    ));
    option.model_limits_enabled = !option.model_limits.is_empty();
    Some(option)
}

pub(super) fn mask_key(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() || key.contains('*') {
        return key.to_string();
    }
    let chars = key.chars().collect::<Vec<_>>();
    if chars.len() <= 2 {
        return format!("{}****", chars.iter().collect::<String>());
    }
    if chars.len() <= 4 {
        return format!(
            "{}****{}",
            chars.first().copied().unwrap_or_default(),
            chars.last().copied().unwrap_or_default()
        );
    }
    if chars.len() <= 8 {
        return format!(
            "{}****{}",
            chars[..2].iter().collect::<String>(),
            chars[chars.len() - 2..].iter().collect::<String>()
        );
    }
    format!(
        "{}********{}",
        chars[..4].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

fn normalize_key_status(value: &Value) -> String {
    let raw = string_field(value, &["status", "state"]).unwrap_or_default();
    match raw.trim().to_ascii_lowercase().as_str() {
        "active" | "enabled" | "1" => "enabled".to_string(),
        "inactive" | "disabled" | "2" => "disabled".to_string(),
        "expired" | "3" => "expired".to_string(),
        "quota_exhausted" | "exhausted" | "4" => "exhausted".to_string(),
        _ => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_key_mapping_preserves_metadata_and_normalizes_status() {
        let option = api_key_from_value(&json!({
            "id": 12,
            "user_id": 7,
            "key": "sk-sub2-secret",
            "name": "主 Key",
            "group": {"id": 3, "name": "premium"},
            "status": "quota_exhausted",
            "quota": 10.0,
            "quota_used": 4.5,
            "ip_whitelist": ["127.0.0.1"],
            "created_at": "2026-01-02T03:04:05Z",
            "expires_at": null,
            "model_limits": ["gpt-4o"]
        }))
        .expect("key should map");

        assert_eq!(option.token_id, "12");
        assert_eq!(option.user_id, "7");
        assert_eq!(option.group, "premium");
        assert_eq!(option.status, "exhausted");
        assert_eq!(option.used_quota, 4.5);
        assert_eq!(option.remain_quota, 5.5);
        assert!(option.key_available);
        assert_eq!(option.allow_ips, vec!["127.0.0.1"]);
        assert_eq!(option.model_limits, vec!["gpt-4o"]);
        assert!(option.created_time.is_some());
        assert!(option.expired_time.is_none());
    }

    #[test]
    fn mask_key_handles_short_values_without_panicking() {
        assert_eq!(mask_key("a"), "a****");
        assert_eq!(mask_key("ab"), "ab****");
        assert_eq!(mask_key("abcd"), "a****d");
        assert_eq!(mask_key(""), "");
    }

    #[test]
    fn api_key_mapping_preserves_a_custom_prefix() {
        let option = api_key_from_value(&json!({
            "id": 13,
            "key": "custom-sub2-key",
            "name": "自定义 Key",
            "status": "active"
        }))
        .expect("key should map");

        assert_eq!(option.key, "custom-sub2-key");
        assert_eq!(option.masked_key, "cust********-key");
    }
}
