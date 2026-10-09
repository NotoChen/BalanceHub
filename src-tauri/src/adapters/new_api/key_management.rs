use std::collections::HashSet;

use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    adapters::transport::ProviderTransport,
    models::{
        remote_api_key_id, Provider, ProviderApiKeyAutoGroups, ProviderApiKeyEditorContext,
        ProviderApiKeyExpiration, ProviderApiKeyGroup, ProviderApiKeyOption, ProviderApiKeyPatch,
        ProviderApiKeySettings, ProviderProtocol,
    },
};

use super::{
    http::{
        build_url, build_user_request, provider_user_management_context, retry_with_access_token,
    },
    keys::{build_api_key_list_url, option_from_value, reveal_api_key},
    response::{extract_string_field, extract_token_items, parse_success_data, send_text},
    site::{fetch_site_metadata, SiteMetadata},
};

/// Retry only the rejected request, never an already completed mutation followed by a read.
async fn request(
    client: &ProviderTransport,
    provider: &Provider,
    method: Method,
    path: &str,
    payload: Option<Value>,
    context: &str,
) -> Result<(Provider, Value), String> {
    let once = |candidate: Provider| {
        let method = method.clone();
        let payload = payload.clone();
        async move {
            let (base_url, user, credential) = provider_user_management_context(&candidate)?;
            let mut request = build_user_request(
                client,
                method,
                build_url(&base_url, path)?,
                &base_url,
                &user,
                credential,
            );
            if let Some(payload) = payload {
                request = request.json(&payload);
            }
            let (status, body) = send_text(client, request, context).await?;
            parse_success_data(&status, body, context)
        }
    };
    retry_with_access_token(client, provider, once(provider.clone()), once).await
}

async fn groups(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<(Provider, Vec<ProviderApiKeyGroup>), String> {
    let (provider, data) = request(
        client,
        provider,
        Method::GET,
        "/api/user/self/groups",
        None,
        "读取可选分组",
    )
    .await?;
    let entries = data.as_object().ok_or("站点未返回可选分组列表")?;
    let groups = entries
        .iter()
        .map(|(name, value)| ProviderApiKeyGroup {
            value: name.clone(),
            label: name.clone(),
            description: extract_string_field(value, &["desc"]).unwrap_or_default(),
            rate: value.get("ratio").and_then(Value::as_f64),
        })
        .collect();
    Ok((provider, groups))
}

async fn auto_groups(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderApiKeyAutoGroups, String> {
    let (_, data) = request(
        client,
        provider,
        Method::GET,
        "/api/token/auto-groups",
        None,
        "读取自动分组范围",
    )
    .await?;
    let groups = data
        .get("groups")
        .and_then(Value::as_array)
        .ok_or("站点未返回自动分组范围")?;
    Ok(ProviderApiKeyAutoGroups {
        groups: groups
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        max_count: data.get("max_count").and_then(Value::as_u64).unwrap_or(0) as usize,
    })
}

pub(super) async fn editor_context(
    client: &ProviderTransport,
    provider: &Provider,
    token_id: Option<&str>,
) -> Result<(Provider, ProviderApiKeyEditorContext), String> {
    let (authenticated, groups) = groups(client, provider).await?;
    let site = fetch_site_metadata(client, &provider.identity.base_url)
        .await
        .map_err(|error| format!("无法确认站点额度单位，请重试：{error}"))?;
    let (authenticated, option) = if let Some(id) = token_id {
        let id = remote_api_key_id(id)?;
        let (next, data) = request(
            client,
            &authenticated,
            Method::GET,
            &format!("/api/token/{id}"),
            None,
            "读取 Key 设置",
        )
        .await?;
        (next, option_from_value(&data, None, &site))
    } else {
        (
            authenticated,
            ProviderApiKeyOption {
                unlimited_quota: true,
                status: "1".into(),
                ..Default::default()
            },
        )
    };
    let (models, auto) = tokio::join!(
        super::models::fetch_account_models(client, &authenticated),
        auto_groups(client, &authenticated),
    );
    let (model_options, model_options_error) = match models {
        Ok((_, models)) => (models, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    Ok((
        authenticated,
        ProviderApiKeyEditorContext {
            credential_revision: provider.auth.credential_revision,
            settings: ProviderApiKeySettings::from_option(&option, ProviderProtocol::NewApi),
            groups,
            group_clearable: true,
            default_group_label: "跟随账号分组".into(),
            quota_label: "剩余额度".into(),
            quota_unit: if site.quota_display_type == "tokens" {
                "Token".into()
            } else {
                site.currency_symbol.clone()
            },
            quota_minimum: 0.0,
            expiration_in_days: false,
            supports_ip_blacklist: false,
            supports_model_limits: true,
            supports_cross_group_retry: true,
            supports_spending_limits: false,
            supports_custom_key: false,
            automatic_group: Some("auto".into()),
            auto_groups: auto.ok(),
            model_options,
            model_options_error,
        },
    ))
}

async fn validate_groups(
    client: &ProviderTransport,
    provider: &Provider,
    patch: &ProviderApiKeyPatch,
) -> Result<Provider, String> {
    let mut authenticated = provider.clone();
    if let Some(group) = patch.group.as_ref().filter(|group| !group.is_empty()) {
        let (next, groups) = groups(client, &authenticated).await?;
        authenticated = next;
        if !groups.iter().any(|candidate| &candidate.value == group) {
            return Err("所选分组不在当前账号可用范围内，请重新打开设置".into());
        }
    }
    if let Some(selected) = &patch.auto_groups {
        let available = auto_groups(client, &authenticated).await?;
        if selected.len() > available.max_count
            || selected.iter().any(|name| !available.groups.contains(name))
        {
            return Err("自动分组范围已变化，请重新选择".into());
        }
        if selected.iter().collect::<HashSet<_>>().len() != selected.len() {
            return Err("自动分组不能重复".into());
        }
    }
    Ok(authenticated)
}

pub(super) fn apply_patch(
    payload: &mut Value,
    patch: &ProviderApiKeyPatch,
    site: &SiteMetadata,
) -> Result<(), String> {
    if let Some(name) = &patch.name {
        payload["name"] = json!(name.trim());
    }
    if let Some(group) = &patch.group {
        payload["group"] = json!(group);
    }
    if let Some(quota) = &patch.quota {
        payload["unlimited_quota"] = json!(quota.unlimited);
        if !quota.unlimited {
            let raw = if site.quota_display_type == "tokens" {
                quota.amount
            } else {
                quota.amount * site.quota_per_unit / site.currency_exchange_rate
            };
            if !raw.is_finite()
                || raw < 0.0
                || raw > (site.quota_per_unit * 1_000_000_000.0).min(i64::MAX as f64)
            {
                return Err("额度超出站点允许的范围".into());
            }
            payload["remain_quota"] = json!(raw.round() as i64);
        }
    }
    if let Some(expiration) = &patch.expiration {
        payload["expired_time"] = json!(match expiration {
            ProviderApiKeyExpiration::Never => -1,
            ProviderApiKeyExpiration::At { timestamp } => timestamp / 1000,
            _ => return Err("NewAPI 有效期需要指定到期时间".into()),
        });
    }
    if let Some(ips) = &patch.allow_ips {
        payload["allow_ips"] = json!(ips
            .iter()
            .map(|ip| ip.trim())
            .collect::<Vec<_>>()
            .join("\n"));
    }
    if let Some(enabled) = patch.model_limits_enabled {
        payload["model_limits_enabled"] = json!(enabled);
    }
    if let Some(models) = &patch.model_limits {
        payload["model_limits"] = json!(models.join(","));
    }
    if let Some(retry) = patch.cross_group_retry {
        payload["cross_group_retry"] = json!(retry);
    }
    if let Some(groups) = &patch.auto_groups {
        payload["auto_groups"] = json!(groups);
    }
    if payload["group"].as_str() != Some("auto") {
        if patch.cross_group_retry == Some(true)
            || patch
                .auto_groups
                .as_ref()
                .is_some_and(|groups| !groups.is_empty())
        {
            return Err("自动分组范围和跨分组重试仅适用于 auto 分组".into());
        }
        payload["cross_group_retry"] = json!(false);
        // Omission lets the server clear this field when leaving the auto group.
        payload.as_object_mut().unwrap().remove("auto_groups");
    }
    Ok(())
}

pub(super) async fn create(
    client: &ProviderTransport,
    provider: &Provider,
    patch: &ProviderApiKeyPatch,
) -> Result<(Provider, ProviderApiKeyOption), String> {
    patch.validate(ProviderProtocol::NewApi, true)?;
    let authenticated = validate_groups(client, provider, patch).await?;
    let site = fetch_site_metadata(client, &provider.identity.base_url).await?;
    let mut payload = json!({"name":"", "group":"", "unlimited_quota":true, "remain_quota":0, "expired_time":-1,
        "model_limits_enabled":false, "model_limits":"", "allow_ips":"", "cross_group_retry":false});
    apply_patch(&mut payload, patch, &site)?;
    // Official NewAPI does not return the new ID. Compare identities before and
    // after creation, never select an existing same-name or unrelated Key.
    let list_url = build_api_key_list_url(&provider.identity.base_url)?;
    let path = format!("/api/token/?{}", list_url.query().unwrap_or_default());
    let (authenticated, before) = request(
        client,
        &authenticated,
        Method::GET,
        &path,
        None,
        "确认创建前的 Key 列表",
    )
    .await?;
    let prior_ids = extract_token_items(&before)
        .iter()
        .filter_map(|value| extract_string_field(value, &["id"]))
        .collect::<HashSet<_>>();
    let (mut authenticated, created) = request(
        client,
        &authenticated,
        Method::POST,
        "/api/token/",
        Some(payload),
        "创建 API Key",
    )
    .await?;
    let readback = async {
        let mut data = if extract_string_field(&created, &["id"]).is_some() {
            created
        } else {
            let (next, after) = request(
                client,
                &authenticated,
                Method::GET,
                &path,
                None,
                "确认新建 Key",
            )
            .await?;
            authenticated = next;
            find_created_token(
                &prior_ids,
                extract_token_items(&after),
                patch.name.as_deref().unwrap_or("").trim(),
            )?
        };
        let id = extract_string_field(&data, &["id"]).ok_or("创建响应缺少 Key ID")?;
        let numeric_id = remote_api_key_id(&id)?;
        if !is_token_snapshot(&data, numeric_id) {
            let (next, details) = request(
                client,
                &authenticated,
                Method::GET,
                &format!("/api/token/{numeric_id}"),
                None,
                "读取新建 Key 设置",
            )
            .await?;
            authenticated = next;
            data = details;
        }
        if !is_token_snapshot(&data, numeric_id) {
            return Err("站点未返回完整的新建 Key 设置".to_string());
        }
        let (base, user, credential) = provider_user_management_context(&authenticated)?;
        let key = extract_string_field(&data, &["key"])
            .filter(|key| crate::models::is_full_api_key_value(key));
        // A successfully created Key can still be managed if reveal is denied.
        let key = match key {
            Some(key) => Some(key),
            None => reveal_api_key(client, &base, &user, credential, &id)
                .await
                .ok(),
        };
        Ok::<_, String>(option_from_value(&data, key, &site))
    }
    .await
    .map_err(|error| {
        format!("Key 已创建，但读取结果失败。请同步站点 Key 后确认，避免重复创建：{error}")
    })?;
    Ok((authenticated, readback))
}

fn find_created_token(
    prior_ids: &HashSet<String>,
    tokens: Vec<Value>,
    name: &str,
) -> Result<Value, String> {
    let mut candidates = tokens.into_iter().filter(|token| {
        extract_string_field(token, &["id"]).is_some_and(|id| !prior_ids.contains(&id))
            && extract_string_field(token, &["name"]).as_deref() == Some(name)
    });
    match (candidates.next(), candidates.next()) {
        (Some(token), None) => Ok(token),
        _ => Err("无法唯一确认新建 Key，未选择其他密钥".into()),
    }
}

pub(super) async fn update(
    client: &ProviderTransport,
    provider: &Provider,
    token_id: &str,
    patch: &ProviderApiKeyPatch,
) -> Result<(Provider, ProviderApiKeyOption), String> {
    patch.validate(ProviderProtocol::NewApi, false)?;
    let id = remote_api_key_id(token_id)?;
    let authenticated = validate_groups(client, provider, patch).await?;
    let site = fetch_site_metadata(client, &provider.identity.base_url).await?;
    let (authenticated, current) = request(
        client,
        &authenticated,
        Method::GET,
        &format!("/api/token/{id}"),
        None,
        "读取最新 Key 设置",
    )
    .await?;
    if extract_string_field(&current, &["id"]).as_deref() != Some(token_id.trim()) {
        return Err("站点返回的 Key ID 不匹配，无法安全更新".into());
    }
    let mut payload = current.clone();
    let object = payload.as_object_mut().ok_or("站点未返回 Key 设置")?;
    for field in [
        "name",
        "remain_quota",
        "unlimited_quota",
        "expired_time",
        "group",
    ] {
        if !object.contains_key(field) {
            return Err(format!("Key 设置缺少 {field}，无法安全更新"));
        }
    }
    // Status is a separate endpoint operation; sending enabled here can reject
    // an expired/exhausted Key before its new quota/expiration is applied.
    for field in [
        "status",
        "key",
        "user_id",
        "used_quota",
        "created_time",
        "accessed_time",
    ] {
        object.remove(field);
    }
    payload["id"] = json!(id);
    let previous_payload = payload.clone();
    apply_patch(&mut payload, patch, &site)?;
    let (mut authenticated, mut updated) = if payload != previous_payload {
        request(
            client,
            &authenticated,
            Method::PUT,
            "/api/token/",
            Some(payload),
            "更新 API Key 设置",
        )
        .await?
    } else {
        (authenticated, current)
    };
    if let Some(enabled) = patch.enabled {
        let (next, result) = request(
            client,
            &authenticated,
            Method::PUT,
            "/api/token/?status_only=true",
            Some(json!({"id":id, "status":if enabled {1} else {2}})),
            "更新 Key 启用状态",
        )
        .await
        .map_err(|error| format!("Key 设置已保存，但启用状态更新失败，请重新同步确认：{error}"))?;
        authenticated = next;
        updated = result;
    }
    if !is_token_snapshot(&updated, id) {
        let (next, result) = request(
            client,
            &authenticated,
            Method::GET,
            &format!("/api/token/{id}"),
            None,
            "确认 Key 更新结果",
        )
        .await
        .map_err(|error| format!("Key 设置已保存，但读取结果失败，请同步确认：{error}"))?;
        authenticated = next;
        updated = result;
    }
    if !is_token_snapshot(&updated, id) {
        return Err("Key 设置已保存，但站点没有返回完整的更新结果，请同步确认".into());
    }
    let option = option_from_value(&updated, None, &site);
    Ok((authenticated, option))
}

fn is_token_snapshot(value: &Value, id: i64) -> bool {
    extract_string_field(value, &["id"]).as_deref() == Some(id.to_string().as_str())
        && [
            "name",
            "remain_quota",
            "unlimited_quota",
            "expired_time",
            "group",
        ]
        .iter()
        .all(|field| value.get(*field).is_some())
}

#[cfg(test)]
#[path = "key_management/tests.rs"]
mod tests;
