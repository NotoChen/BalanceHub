//! NewAPI 站点价目。
//!
//! `/api/pricing` 返回的是模型倍率，不是直接的美元价格：输入单价按
//! `model_ratio * 1_000_000 / quota_per_unit` 换算，再乘账号分组倍率；
//! `quota_type = 1` 的模型按次计费，价格是 `model_price * 分组倍率`。
//! 分组倍率和可选分组在响应顶层，与 `data` 并列，不能只解包 `data`。

use crate::{
    adapters::transport::ProviderTransport,
    limits,
    models::{
        Provider, ProviderModelPrice, ProviderOptionalCapability, ProviderOptionalCapabilityState,
        ProviderPricingSnapshot, ProviderPricingSource,
    },
};
use reqwest::header::{ACCEPT, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

use super::http::{
    apply_auth_headers, apply_session_cookie, build_url, normalize_base_url, USER_AGENT_VALUE,
};
use super::response::{extract_f64_field, extract_string_field, send_text, trim_message};
use super::site::{fetch_site_metadata_or, site_metadata_from_provider};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct PricingProbe {
    pub snapshot: ProviderPricingSnapshot,
    pub state: ProviderOptionalCapabilityState,
}

enum PricingFailure {
    /// 站点明确没有这个接口或没有开启价目功能。这是能力结论，不是请求失败。
    Unsupported(String),
    /// 网络、认证或响应格式问题；保留旧快照，不把能力标记为不支持。
    Failed(String),
}

/// 读取价目；能力探测和普通刷新共用。网络或认证失败保留上一次成功价格，
/// 站点明确不支持时清空旧价格。
pub async fn probe_pricing(client: &ProviderTransport, provider: &Provider) -> PricingProbe {
    let mut snapshot = provider.capabilities.pricing.clone();
    match fetch_pricing(client, provider).await {
        Ok(loaded) => {
            let count = loaded.models.len();
            let group = loaded.group.clone();
            snapshot.mark_loaded(
                ProviderPricingSource::NewApi,
                loaded.models,
                loaded.groups,
                group.clone(),
            );
            let reason = if group.is_empty() {
                format!("已读取 {count} 个模型的站点价格")
            } else {
                format!("已读取 {count} 个模型的站点价格（分组 {group}）")
            };
            PricingProbe {
                snapshot,
                state: ProviderOptionalCapabilityState::supported(
                    ProviderOptionalCapability::Pricing,
                    reason,
                ),
            }
        }
        Err(PricingFailure::Unsupported(reason)) => {
            snapshot.mark_unsupported(reason.clone());
            PricingProbe {
                snapshot,
                state: ProviderOptionalCapabilityState::unsupported(
                    ProviderOptionalCapability::Pricing,
                    reason,
                ),
            }
        }
        Err(PricingFailure::Failed(error)) => {
            snapshot.mark_error(error.clone());
            PricingProbe {
                snapshot,
                state: ProviderOptionalCapabilityState::unknown(
                    ProviderOptionalCapability::Pricing,
                    error,
                ),
            }
        }
    }
}

struct LoadedPricing {
    models: BTreeMap<String, ProviderModelPrice>,
    groups: BTreeMap<String, f64>,
    group: String,
}

async fn fetch_pricing(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<LoadedPricing, PricingFailure> {
    let base_url = normalize_base_url(&provider.identity.base_url);
    if base_url.is_empty() {
        return Err(PricingFailure::Failed("缺少中转站地址".to_string()));
    }
    let site = fetch_site_metadata_or(client, &base_url, site_metadata_from_provider(provider))
        .await
        .map_err(PricingFailure::Failed)?;
    let quota_per_unit = site.quota_per_unit;
    if !quota_per_unit.is_finite() || quota_per_unit <= 0.0 {
        return Err(PricingFailure::Failed(
            "站点没有返回有效的额度单位，无法换算价格".to_string(),
        ));
    }

    let url = build_url(&base_url, "/api/pricing").map_err(PricingFailure::Failed)?;
    let (status, body) = send_pricing(client, provider, url, &base_url, true).await?;
    let (status, body) = if matches!(status.as_u16(), 401 | 403) {
        // 价目接口在多数站点是公开的；带账号凭据被拒时用匿名请求再试一次，
        // 不把认证方式差异当成站点不支持价格。
        let url = build_url(&base_url, "/api/pricing").map_err(PricingFailure::Failed)?;
        send_pricing(client, provider, url, &base_url, false).await?
    } else {
        (status, body)
    };

    if matches!(status.as_u16(), 404 | 501) {
        return Err(PricingFailure::Unsupported(format!(
            "站点未提供价目接口（HTTP {}）",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        return Err(PricingFailure::Failed(format!(
            "HTTP {}: {}",
            status.as_u16(),
            trim_message(&body)
        )));
    }
    let decoded = serde_json::from_str::<Value>(&body)
        .map_err(|error| PricingFailure::Failed(format!("解析站点价目失败: {error}")))?;
    if decoded.get("success").and_then(Value::as_bool) != Some(true) {
        let message = decoded
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("接口返回失败")
            .trim()
            .to_string();
        return if looks_unsupported(&message) {
            Err(PricingFailure::Unsupported(message))
        } else {
            Err(PricingFailure::Failed(message))
        };
    }
    let Some(items) = decoded.get("data").and_then(Value::as_array) else {
        return Err(PricingFailure::Unsupported(
            "站点价目接口没有返回模型列表".to_string(),
        ));
    };

    let groups = decoded
        .get("group_ratio")
        .and_then(Value::as_object)
        .map(|groups| {
            groups
                .iter()
                .filter_map(|(name, ratio)| {
                    let ratio = ratio
                        .as_f64()
                        .filter(|ratio| ratio.is_finite() && *ratio >= 0.0)?;
                    Some((name.clone(), ratio))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let usable_groups = decoded
        .get("usable_group")
        .and_then(Value::as_object)
        .map(|groups| groups.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let group = effective_group(provider, &groups, &usable_groups);
    let ratio = groups.get(&group).copied().unwrap_or(1.0);

    let mut models = Vec::new();
    for item in items {
        let Some(name) = extract_string_field(item, &["model_name", "modelName"]) else {
            continue;
        };
        if !group_enabled(item, &group) {
            continue;
        }
        let Some(price) = model_price(item, ratio, quota_per_unit, &group) else {
            continue;
        };
        models.push((name, price));
    }
    models.sort_by(|left, right| left.0.cmp(&right.0));
    models.truncate(limits::MAX_MODELS_PER_PROVIDER);

    Ok(LoadedPricing {
        models: models.into_iter().collect(),
        groups,
        group,
    })
}

async fn send_pricing(
    client: &ProviderTransport,
    provider: &Provider,
    url: reqwest::Url,
    base_url: &str,
    authenticated: bool,
) -> Result<(reqwest::StatusCode, String), PricingFailure> {
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"))
        .timeout(REQUEST_TIMEOUT);
    if authenticated {
        request = apply_auth_headers(request, provider);
        request = apply_session_cookie(request, provider);
    }
    send_text(client, request, "读取站点价目")
        .await
        .map_err(PricingFailure::Failed)
}

fn effective_group(
    provider: &Provider,
    groups: &BTreeMap<String, f64>,
    usable_groups: &[String],
) -> String {
    let candidates = [
        provider.quota.group.trim().to_string(),
        provider
            .auth
            .api_key_options
            .iter()
            .find(|option| {
                option.key.trim() == provider.auth.api_key.trim() && !option.group.trim().is_empty()
            })
            .map(|option| option.group.trim().to_string())
            .unwrap_or_default(),
        provider
            .auth
            .api_key_options
            .iter()
            .find(|option| !option.group.trim().is_empty())
            .map(|option| option.group.trim().to_string())
            .unwrap_or_default(),
    ];
    for candidate in candidates {
        if candidate.is_empty() {
            continue;
        }
        if groups.contains_key(&candidate) || usable_groups.iter().any(|group| group == &candidate)
        {
            return candidate;
        }
    }
    if groups.contains_key("default") {
        return "default".to_string();
    }
    groups
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

/// `enable_groups` 为空表示所有分组可用；`all` 是站点的通配写法。
fn group_enabled(item: &Value, group: &str) -> bool {
    let Some(groups) = item.get("enable_groups").and_then(Value::as_array) else {
        return true;
    };
    if groups.is_empty() {
        return true;
    }
    groups
        .iter()
        .filter_map(Value::as_str)
        .any(|enabled| enabled == "all" || enabled == group)
}

fn model_price(
    item: &Value,
    group_ratio: f64,
    quota_per_unit: f64,
    group: &str,
) -> Option<ProviderModelPrice> {
    let quota_type = item.get("quota_type").and_then(Value::as_i64).unwrap_or(0);
    if quota_type == 1 {
        let price = extract_f64_field(item, &["model_price", "modelPrice"])
            .filter(|price| price.is_finite() && *price >= 0.0)
            .map(|price| price * group_ratio)?;
        return Some(ProviderModelPrice {
            per_request: Some(price),
            group: group.to_string(),
            ..ProviderModelPrice::default()
        });
    }
    let model_ratio = extract_f64_field(item, &["model_ratio", "modelRatio"])
        .filter(|ratio| ratio.is_finite() && *ratio >= 0.0)?;
    let input = model_ratio * 1_000_000.0 / quota_per_unit * group_ratio;
    if !input.is_finite() || input < 0.0 {
        return None;
    }
    let completion_ratio = extract_f64_field(item, &["completion_ratio", "completionRatio"])
        .filter(|ratio| ratio.is_finite() && *ratio >= 0.0)
        .unwrap_or(1.0);
    let cache_ratio = extract_f64_field(item, &["cache_ratio", "cacheRatio"])
        .filter(|ratio| ratio.is_finite() && *ratio >= 0.0);
    let create_cache_ratio = extract_f64_field(item, &["create_cache_ratio", "createCacheRatio"])
        .filter(|ratio| ratio.is_finite() && *ratio >= 0.0);
    Some(ProviderModelPrice {
        input: Some(input),
        output: Some(input * completion_ratio),
        cache_read: cache_ratio.map(|ratio| input * ratio),
        cache_write: create_cache_ratio.map(|ratio| input * ratio),
        per_request: None,
        group: group.to_string(),
    })
}

fn looks_unsupported(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("not found")
        || normalized.contains("not enabled")
        || normalized.contains("not supported")
        || normalized.contains("disabled")
        || message.contains("未开启")
        || message.contains("不支持")
        || message.contains("不存在")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ratio_prices_are_converted_to_usd_per_million_with_group_multiplier() {
        let price = model_price(
            &json!({
                "model_ratio": 1.0,
                "completion_ratio": 4.0,
                "cache_ratio": 0.1,
                "create_cache_ratio": 1.25
            }),
            1.5,
            500_000.0,
            "vip",
        )
        .expect("ratio model should have a price");
        // 1 * 1e6 / 500000 = $2/M，再乘分组 1.5。
        assert_eq!(price.input, Some(3.0));
        assert_eq!(price.output, Some(12.0));
        // 倍率换算会带浮点尾差，按金额精度比较。
        let close = |value: Option<f64>, expected: f64| {
            (value.expect("cache price") - expected).abs() < 1e-9
        };
        assert!(close(price.cache_read, 0.3));
        assert!(close(price.cache_write, 3.75));
        assert_eq!(price.group, "vip");
    }

    #[test]
    fn per_request_models_keep_their_own_billing_unit() {
        let price = model_price(
            &json!({ "quota_type": 1, "model_price": 0.02 }),
            2.0,
            500_000.0,
            "vip",
        )
        .expect("per-request model should have a price");
        assert_eq!(price.per_request, Some(0.04));
        assert_eq!(price.input, None);
        assert!(!price.has_token_price());
    }

    #[test]
    fn models_logically_excluded_from_the_group_are_skipped() {
        assert!(group_enabled(
            &json!({ "enable_groups": ["default", "vip"] }),
            "vip"
        ));
        assert!(group_enabled(&json!({ "enable_groups": ["all"] }), "vip"));
        assert!(!group_enabled(&json!({ "enable_groups": ["svip"] }), "vip"));
        assert!(group_enabled(&json!({}), "vip"));
    }

    #[test]
    fn effective_group_prefers_the_stored_account_group() {
        let mut provider = Provider::from_input(Default::default(), "p1".into());
        provider.quota.group = "vip".into();
        let groups = BTreeMap::from([("default".to_string(), 1.0), ("vip".to_string(), 1.5)]);
        assert_eq!(
            effective_group(&provider, &groups, &["default".into(), "vip".into()]),
            "vip"
        );
        provider.quota.group.clear();
        assert_eq!(effective_group(&provider, &groups, &[]), "default");
    }
}
