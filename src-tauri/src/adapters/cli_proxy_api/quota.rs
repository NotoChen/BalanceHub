//! Reading one credential's quota **through the gateway**.
//!
//! The provider is asked by a plugin running on the gateway, which normalizes
//! the answer into a plan, a few summary numbers and limit windows. This app
//! only relays that answer: nothing is stored and no renewal is touched.
use super::{base_url, identifier, require_key};
use crate::adapters::oauth::http;
use crate::models::*;
use crate::network;
use reqwest::{Client, Response};
use serde_json::Value;

const CONTEXT: &str = "读取 CPA 授权额度";

/// Reads the quota of one credential. A v8 gateway names the plugin that owns
/// the credential, and only that route exists there; the generic route lives on
/// v0 and also resolves the declarative probe a v0 gateway reports instead.
pub(crate) async fn account_quota(
    client: &Client,
    provider: &Provider,
    auth_index: &str,
    plugin_id: &str,
    api_version: &str,
) -> Result<CpaAccountQuota, String> {
    require_key(provider)?;
    let index = identifier(auth_index, "CPA 授权索引无效")?;
    let plugin = match plugin_id.trim() {
        "" => None,
        raw => Some(identifier(raw, "CPA 插件标识无效")?),
    };
    let base = base_url(provider)?;
    let response = match (&plugin, api_version) {
        (Some(id), "v8") => {
            let response = client
                .get(format!("{base}/v8/management/plugins/{id}/quota"))
                .query(&[("auth_index", index.as_str())])
                .bearer_auth(&provider.auth.access_token)
                .send()
                .await
                .map_err(|_| "连接 CPA 管理接口失败")?;
            // A gateway that was downgraded between the list and this read
            // still answers on v0.
            match response.status().as_u16() {
                404 | 405 => generic_quota(client, &base, &index, provider).await?,
                _ => response,
            }
        }
        _ => generic_quota(client, &base, &index, provider).await?,
    };
    quota_value(response).await.map(|value| parse_quota(&value))
}

/// The v0 route both plugin and declarative quotas go through.
async fn generic_quota(
    client: &Client,
    base: &str,
    index: &str,
    provider: &Provider,
) -> Result<Response, String> {
    client
        .post(format!("{base}/v0/management/quota/fetch"))
        .bearer_auth(&provider.auth.access_token)
        .json(&serde_json::json!({ "auth_index": index }))
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败".to_string())
}

/// Reads the answer and keeps the gateway's own reason: a quota request is the
/// one management call whose failure explains the upstream state, so a message
/// the panel does not know is passed through instead of replaced by a status.
async fn quota_value(response: Response) -> Result<Value, String> {
    let status = response.status();
    let text = network::read_http_text(response, CONTEXT)
        .await
        .map_err(|_| format!("{CONTEXT}响应读取失败"))?;
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(value);
    }
    if matches!(status.as_u16(), 401 | 403) {
        return Err(http::http_error(CONTEXT, status.as_u16()));
    }
    Err(quota_reason(&value).unwrap_or_else(|| http::http_error(CONTEXT, status.as_u16())))
}

/// The reasons CPA gives when it cannot read a credential's quota, in the
/// panel's words. An upstream failure keeps its detail because it names what
/// the provider refused.
fn quota_reason(value: &Value) -> Option<String> {
    let message = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())?;
    if let Some(detail) = message.strip_prefix("failed to fetch quota: ") {
        let detail = detail.trim();
        return Some(if detail.is_empty() {
            "CPA 读取额度失败".into()
        } else {
            format!("CPA 读取额度失败：{detail}")
        });
    }
    Some(
        match message {
            "auth not found" => "CPA 上找不到该授权，请刷新后重试",
            "quota provider not found for plugin" => "该插件不提供额度查询",
            "auth_index is required" => "该授权没有可供查询的索引",
            "invalid request body" => "额度请求格式无效",
            "upstream probe response does not match normalized quota shape or declared mapping" => {
                "CPA 返回的额度数据无法识别"
            }
            _ => return None,
        }
        .into(),
    )
}

/// Reads the normalized shape a plugin returns, tolerating the parts it left
/// out: a summary metric without a label or a bucket without a fraction is
/// dropped rather than shown as a made-up value.
fn parse_quota(value: &Value) -> CpaAccountQuota {
    let subscription = value.get("subscription");
    let plan = subscription
        .map(|s| http::text(s, &["plan"]))
        .unwrap_or_default();
    let tier = subscription
        .map(|s| http::text(s, &["tierName", "tier_name"]))
        .unwrap_or_default();
    CpaAccountQuota {
        plan,
        tier,
        metrics: value
            .get("summary")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(quota_metric).collect())
            .unwrap_or_default(),
        groups: value
            .get("groups")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(quota_group).collect())
            .unwrap_or_default(),
    }
}

fn quota_metric(value: &Value) -> Option<CpaQuotaMetric> {
    let key = http::text(value, &["key"]);
    let label = http::text(value, &["label"]);
    let number = value.get("value").and_then(Value::as_f64)?;
    // The panel has nothing to label a nameless number with.
    if key.is_empty() || label.is_empty() || !number.is_finite() {
        return None;
    }
    Some(CpaQuotaMetric {
        key,
        label,
        value: number,
        unit: http::text(value, &["unit"]),
        format: http::text(value, &["format"]),
        currency: http::text(value, &["currency"]).to_ascii_uppercase(),
    })
}

fn quota_group(value: &Value) -> CpaQuotaGroup {
    CpaQuotaGroup {
        display_name: http::text(value, &["displayName", "display_name"]),
        buckets: value
            .get("buckets")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(quota_bucket).collect())
            .unwrap_or_default(),
    }
}

fn quota_bucket(value: &Value) -> CpaQuotaBucket {
    CpaQuotaBucket {
        window: http::text(value, &["window"]),
        remaining_fraction: value
            .get("remainingFraction")
            .or_else(|| value.get("remaining_fraction"))
            .and_then(Value::as_f64)
            // A fraction outside 0–1 would draw a bar past its track.
            .filter(|fraction| (0.0..=1.0).contains(fraction)),
        reset_at: super::cpa_time(value, "resetTime")
            .or_else(|| super::cpa_time(value, "reset_time")),
        description: http::text(value, &["description"]),
    }
}
