use reqwest::Method;
use serde_json::{json, Value};

use super::{
    auth::request_account_json,
    json::{array_items, string_field},
    keys::api_key_from_value,
};
use crate::{
    adapters::transport::ProviderTransport,
    models::{
        remote_api_key_id, Provider, ProviderApiKeyEditorContext, ProviderApiKeyExpiration,
        ProviderApiKeyGroup, ProviderApiKeyOption, ProviderApiKeyPatch, ProviderApiKeySettings,
        ProviderProtocol,
    },
};

async fn groups(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<(Provider, Vec<ProviderApiKeyGroup>), String> {
    let (provider, data) = request_account_json(
        client,
        provider,
        Method::GET,
        "/groups/available",
        None,
        "读取可选分组",
    )
    .await?;
    if !data.is_array() && !data.get("items").is_some_and(Value::is_array) {
        return Err("站点未返回可选分组列表".into());
    }
    let groups = array_items(&data)
        .iter()
        .filter_map(|group| {
            Some(ProviderApiKeyGroup {
                value: string_field(group, &["id"])?,
                label: string_field(group, &["name"]).unwrap_or_default(),
                description: string_field(group, &["description", "platform"]).unwrap_or_default(),
                // The effective user rate may override group.rate_multiplier.
                rate: None,
            })
        })
        .collect();
    Ok((provider, groups))
}

pub(super) async fn editor_context(
    client: &ProviderTransport,
    provider: &Provider,
    token_id: Option<&str>,
) -> Result<(Provider, ProviderApiKeyEditorContext), String> {
    let (authenticated, groups) = groups(client, provider).await?;
    let (authenticated, option) = if let Some(id) = token_id {
        let id = remote_api_key_id(id)?;
        let (next, value) = request_account_json(
            client,
            &authenticated,
            Method::GET,
            &format!("/keys/{id}"),
            None,
            "读取 Key 设置",
        )
        .await?;
        (next, require_key(&value)?)
    } else {
        (
            authenticated,
            ProviderApiKeyOption {
                unlimited_quota: true,
                status: "enabled".into(),
                ..Default::default()
            },
        )
    };
    Ok((
        authenticated,
        ProviderApiKeyEditorContext {
            credential_revision: provider.auth.credential_revision,
            settings: ProviderApiKeySettings::from_option(&option, ProviderProtocol::Sub2Api),
            groups,
            group_clearable: token_id.is_none() || option.group_id.is_empty(),
            default_group_label: "不绑定分组".into(),
            quota_label: "总额度".into(),
            quota_unit: "USD".into(),
            quota_minimum: 0.000001,
            expiration_in_days: token_id.is_none(),
            supports_ip_blacklist: true,
            supports_model_limits: false,
            supports_cross_group_retry: false,
            automatic_group: None,
            supports_spending_limits: true,
            supports_custom_key: token_id.is_none(),
            auto_groups: None,
            model_options: Vec::new(),
            model_options_error: None,
        },
    ))
}

fn require_key(value: &Value) -> Result<ProviderApiKeyOption, String> {
    api_key_from_value(value)
        .filter(|option| !option.token_id.is_empty())
        .ok_or_else(|| "站点未返回有效的 API Key 元数据".into())
}

async fn validate_group(
    client: &ProviderTransport,
    provider: &Provider,
    patch: &ProviderApiKeyPatch,
) -> Result<Provider, String> {
    let Some(group) = patch.group.as_ref().filter(|group| !group.is_empty()) else {
        return Ok(provider.clone());
    };
    remote_api_key_id(group)?;
    let (authenticated, available) = groups(client, provider).await?;
    if !available.iter().any(|candidate| &candidate.value == group) {
        return Err("所选分组不在当前账号可用范围内，请重新打开设置".into());
    }
    Ok(authenticated)
}

pub(super) fn payload(patch: &ProviderApiKeyPatch, creating: bool) -> Result<Value, String> {
    patch.validate(ProviderProtocol::Sub2Api, creating)?;
    let mut result = json!({});
    if let Some(name) = &patch.name {
        result["name"] = json!(name.trim());
    }
    if let Some(group) = &patch.group {
        if group.is_empty() {
            if !creating {
                return Err("Sub2API 不支持解除已有分组，请选择另一个可用分组".into());
            }
        } else {
            result["group_id"] = json!(remote_api_key_id(group)?);
        }
    }
    if let Some(quota) = &patch.quota {
        result["quota"] = json!(if quota.unlimited { 0.0 } else { quota.amount });
    }
    if let Some(expiration) = &patch.expiration {
        match expiration {
            ProviderApiKeyExpiration::Never if !creating => result["expires_at"] = json!(""),
            ProviderApiKeyExpiration::AfterDays { days } => result["expires_in_days"] = json!(days),
            ProviderApiKeyExpiration::At { timestamp } => {
                result["expires_at"] = json!(chrono::DateTime::from_timestamp_millis(*timestamp)
                    .ok_or("到期时间无效")?
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            }
            ProviderApiKeyExpiration::Never => {}
        }
    }
    if let Some(ips) = &patch.allow_ips {
        result["ip_whitelist"] = json!(ips.iter().map(|ip| ip.trim()).collect::<Vec<_>>());
    }
    if let Some(ips) = &patch.deny_ips {
        result["ip_blacklist"] = json!(ips.iter().map(|ip| ip.trim()).collect::<Vec<_>>());
    }
    if let Some(limits) = &patch.spending_limits {
        result["rate_limit_5h"] = json!(limits.five_hours);
        result["rate_limit_1d"] = json!(limits.one_day);
        result["rate_limit_7d"] = json!(limits.seven_days);
    }
    if !creating {
        if let Some(enabled) = patch.enabled {
            result["status"] = json!(if enabled { "active" } else { "inactive" });
        }
    }
    if let Some(key) = &patch.custom_key {
        if !key.is_empty() {
            result["custom_key"] = json!(key);
        }
    }
    Ok(result)
}

pub(super) async fn create(
    client: &ProviderTransport,
    provider: &Provider,
    patch: &ProviderApiKeyPatch,
) -> Result<(Provider, ProviderApiKeyOption), String> {
    let payload = payload(patch, true)?;
    let authenticated = validate_group(client, provider, patch).await?;
    let (authenticated, value) = request_account_json(
        client,
        &authenticated,
        Method::POST,
        "/keys",
        Some(payload),
        "创建 API Key",
    )
    .await?;
    let option = require_key(&value).map_err(|error| {
        format!("Key 已创建，但结果未能读取。请先同步站点 Key，避免重复创建：{error}")
    })?;
    Ok((authenticated, option))
}

pub(super) async fn update(
    client: &ProviderTransport,
    provider: &Provider,
    token_id: &str,
    patch: &ProviderApiKeyPatch,
) -> Result<(Provider, ProviderApiKeyOption), String> {
    let id = remote_api_key_id(token_id)?;
    let payload = payload(patch, false)?;
    let authenticated = validate_group(client, provider, patch).await?;
    let (authenticated, value) = request_account_json(
        client,
        &authenticated,
        Method::PUT,
        &format!("/keys/{id}"),
        Some(payload),
        "更新 API Key 设置",
    )
    .await?;
    let option = require_key(&value)
        .map_err(|error| format!("Key 设置已保存，但结果未能读取。请同步站点 Key 确认：{error}"))?;
    Ok((authenticated, option))
}

#[cfg(test)]
#[path = "key_management/tests.rs"]
mod tests;
