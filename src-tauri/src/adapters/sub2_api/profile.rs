use crate::models::{Provider, ProviderQuotaDisplay, ProviderQuotaScope};
use reqwest::{Client, Method};
use serde_json::Value;
use std::time::Duration;

use super::{
    json::{number_field, string_field},
    response::{api_url, gateway_url, request_json, Credential},
};

pub(super) async fn fetch_site(client: &Client, base_url: &str) -> Result<Value, String> {
    let site = super::response::request_json_with_timeout(
        client,
        Method::GET,
        api_url(base_url, "/settings/public")?,
        None,
        None,
        "读取 Sub2API 站点信息",
        Duration::from_secs(8),
    )
    .await?;
    if !is_sub2_api_public_settings(&site) {
        return Err("响应不符合 Sub2API 公开设置结构".to_string());
    }
    Ok(site)
}

fn is_sub2_api_public_settings(value: &Value) -> bool {
    let has_name = value.get("site_name").is_some() || value.get("siteName").is_some();
    let has_protocol_marker = [
        "registration_enabled",
        "registrationEnabled",
        "api_base_url",
        "apiBaseUrl",
        "password_reset_enabled",
        "version",
    ]
    .iter()
    .any(|field| value.get(*field).is_some());

    has_name && has_protocol_marker
}

pub(super) async fn fetch_models(
    client: &Client,
    provider: &Provider,
) -> Result<Vec<String>, String> {
    let key = provider.auth.api_key.trim();
    if key.is_empty() {
        return Err("缺少 API Key，无法读取 Sub2API 模型列表".to_string());
    }
    let value = request_json(
        client,
        Method::GET,
        gateway_url(&provider.identity.base_url, "/v1/models")?,
        Some(Credential::ApiKey(key.to_string())),
        None,
        "读取 Sub2API 模型列表",
    )
    .await?;
    Ok(value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| string_field(item, &["id", "name"]))
        .collect())
}

pub(super) fn apply_user(provider: &mut Provider, user: &Value) {
    provider.identity.user_id =
        string_field(user, &["id", "user_id", "userId"]).unwrap_or_default();
    let username = string_field(user, &["username"]).unwrap_or_default();
    let email = string_field(user, &["email"]).unwrap_or_default();
    provider.identity.display_name = string_field(user, &["display_name", "displayName"])
        .or_else(|| (!username.is_empty()).then_some(username.clone()))
        .or_else(|| (!email.is_empty()).then_some(email.clone()))
        .unwrap_or_default();
    provider.identity.username = if !email.is_empty() { email } else { username };
    if provider.auth.login_username.trim().is_empty() {
        if let Some(name) = user_login_name(user) {
            provider.auth.login_username = name;
        }
    }
    provider.quota.available = number_field(user, &["balance"]);
    provider.quota.used = 0.0;
    provider.quota.known = true;
    provider.quota.total_known = false;
    provider.quota.scope = ProviderQuotaScope::Account;
    provider.quota.display_type = "currency".to_string();
    provider.quota.currency_symbol = "$".to_string();
}

pub(super) fn user_login_name(user: &Value) -> Option<String> {
    ["email", "username"]
        .iter()
        .find_map(|field| string_field(user, &[*field]))
}

pub(super) fn quota_display(provider: &Provider) -> ProviderQuotaDisplay {
    ProviderQuotaDisplay {
        quota_display_type: provider.quota.display_type.clone(),
        currency_symbol: provider.quota.currency_symbol.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderProtocol};
    use serde_json::json;

    #[test]
    fn apply_user_keeps_username_and_email_visible_and_does_not_fake_usage() {
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Sub2Api;
        input.identity.base_url = "https://sub2.example".to_string();
        let mut provider = Provider::from_input(input, "sub2-test".to_string());

        apply_user(
            &mut provider,
            &json!({
                "id": 42,
                "username": "alice",
                "email": "alice@example.com",
                "balance": 12.5,
                "frozen_balance": 99.0
            }),
        );

        assert_eq!(provider.identity.display_name, "alice");
        assert_eq!(provider.identity.username, "alice@example.com");
        assert_eq!(provider.identity.user_id, "42");
        assert_eq!(provider.quota.available, 12.5);
        assert_eq!(provider.quota.used, 0.0);
        assert!(!provider.quota.total_known);
    }

    #[test]
    fn public_settings_signature_requires_name_and_known_marker() {
        assert!(is_sub2_api_public_settings(&json!({
            "site_name": "Sub2API",
            "registration_enabled": true,
            "version": "1.0.0"
        })));
        assert!(is_sub2_api_public_settings(&json!({
            "siteName": "Sub2API",
            "apiBaseUrl": "https://api.example.com"
        })));
        assert!(!is_sub2_api_public_settings(&json!({
            "site_name": "Generic site"
        })));
    }
}
