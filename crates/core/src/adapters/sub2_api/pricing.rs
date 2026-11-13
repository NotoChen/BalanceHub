//! Sub2API 模型广场价目。
//!
//! `/api/v1/model-plaza` 以分组为顶层，`pricing` 已经是实收口径的 USD / token
//! 单价；这里统一乘 1e6 转成 USD / 百万 token。按次计费保留 `per_request_price`，
//! 不与 token 单价混算。广场受站点功能开关控制，关闭时返回 404。

use crate::{
    adapters::transport::ProviderTransport,
    limits,
    models::{
        Provider, ProviderModelPrice, ProviderOptionalCapability, ProviderOptionalCapabilityState,
        ProviderPricingSnapshot, ProviderPricingSource,
    },
};
use reqwest::Method;
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

use super::json::string_field;
use super::response::{api_url, request_json_with_timeout, Credential};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct PricingProbe {
    pub snapshot: ProviderPricingSnapshot,
    pub state: ProviderOptionalCapabilityState,
}

enum PricingFailure {
    Unsupported(String),
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
                ProviderPricingSource::Sub2Api,
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
    let url = api_url(&provider.identity.base_url, "/model-plaza")
        .map_err(|error| PricingFailure::Failed(error.to_string()))?;
    let credential = (!provider.auth.access_token.trim().is_empty())
        .then(|| Credential::Jwt(provider.auth.access_token.clone()));
    let mut result = request_json_with_timeout(
        client,
        Method::GET,
        url.clone(),
        credential.clone(),
        None,
        "读取模型广场价目",
        REQUEST_TIMEOUT,
    )
    .await;
    if credential.is_some()
        && result
            .as_ref()
            .err()
            .is_some_and(|error| error.starts_with("HTTP 401") || error.starts_with("HTTP 403"))
    {
        // 模型广场是公开接口；过期账号令牌不应把公开价目挡成不可用。
        result = request_json_with_timeout(
            client,
            Method::GET,
            url,
            None,
            None,
            "读取模型广场价目",
            REQUEST_TIMEOUT,
        )
        .await;
    }
    let data = result.map_err(classify_error)?;

    let groups = data
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| PricingFailure::Unsupported("站点没有返回模型广场分组".to_string()))?;
    let account_group = fetch_account_group(client, provider).await;
    let key_group = current_key_group(provider);
    let selected = select_group(groups, &account_group, &key_group);
    let group = selected
        .and_then(|index| groups.get(index))
        .ok_or_else(|| PricingFailure::Unsupported("当前账号没有可见的模型分组".to_string()))?;
    let group_name = string_field(group, &["name"]).unwrap_or_default();

    let groups_map = groups
        .iter()
        .filter_map(|item| {
            let name = string_field(item, &["name"])?;
            let rate = item
                .get("rate_multiplier")
                .or_else(|| item.get("rateMultiplier"))
                .and_then(Value::as_f64)
                .filter(|rate| rate.is_finite())?;
            Some((name, rate))
        })
        .collect::<BTreeMap<_, _>>();

    let mut models = Vec::new();
    for item in group
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = string_field(item, &["name", "model_name", "modelName"]) else {
            continue;
        };
        let Some(price) = model_price(item, &group_name) else {
            continue;
        };
        models.push((name, price));
    }
    models.sort_by(|left, right| left.0.cmp(&right.0));
    models.truncate(limits::MAX_MODELS_PER_PROVIDER);

    Ok(LoadedPricing {
        models: models.into_iter().collect(),
        groups: groups_map,
        group: group_name,
    })
}

/// 订阅账号的实际计费分组是当前订阅的分组；读不到就退回 Key 分组，再退回
/// 广场里的第一个可见分组。整个请求失败不影响价目本身。
async fn fetch_account_group(client: &ProviderTransport, provider: &Provider) -> String {
    let token = provider.auth.access_token.trim();
    if token.is_empty() {
        return String::new();
    }
    let Ok(url) = api_url(&provider.identity.base_url, "/subscriptions/progress") else {
        return String::new();
    };
    let Ok(data) = request_json_with_timeout(
        client,
        Method::GET,
        url,
        Some(Credential::Jwt(token.to_string())),
        None,
        "读取订阅分组",
        Duration::from_secs(8),
    )
    .await
    else {
        return String::new();
    };
    data.as_array()
        .into_iter()
        .flatten()
        .find_map(|item| {
            let progress = item.get("progress").unwrap_or(item);
            string_field(progress, &["group_name", "groupName"])
        })
        .unwrap_or_default()
}

fn current_key_group(provider: &Provider) -> String {
    let current = provider.auth.api_key.trim();
    provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key.trim() == current && !option.group.trim().is_empty())
        .or_else(|| {
            provider
                .auth
                .api_key_options
                .iter()
                .find(|option| !option.group.trim().is_empty())
        })
        .map(|option| option.group.trim().to_string())
        .or_else(|| {
            let group = provider.quota.group.trim();
            (!group.is_empty()).then(|| group.to_string())
        })
        .unwrap_or_default()
}

fn select_group(groups: &[Value], account_group: &str, key_group: &str) -> Option<usize> {
    let find = |name: &str| {
        if name.trim().is_empty() {
            return None;
        }
        // Key 绑定的分组可能以名称或站点 ID 存储，两种都要能对上。
        groups.iter().position(|group| {
            ["name", "id"]
                .into_iter()
                .any(|field| string_field(group, &[field]).is_some_and(|value| value == name))
        })
    };
    find(account_group)
        .or_else(|| find(key_group))
        .or_else(|| (!groups.is_empty()).then_some(0))
}

fn model_price(item: &Value, group: &str) -> Option<ProviderModelPrice> {
    let pricing = item.get("pricing").filter(|pricing| pricing.is_object())?;
    let billing_mode = string_field(pricing, &["billing_mode", "billingMode"]).unwrap_or_default();
    if billing_mode == "image" {
        // 图片计费按张计价，token 估算无法表示；留给模型详情原样说明。
        return None;
    }
    let per_request = pricing
        .get("per_request_price")
        .and_then(Value::as_f64)
        .filter(|price| price.is_finite() && *price >= 0.0);
    if per_request.is_some() || billing_mode == "per_request" {
        return Some(ProviderModelPrice {
            per_request,
            group: group.to_string(),
            ..ProviderModelPrice::default()
        });
    }
    let price = |fields: &[&str]| {
        fields
            .iter()
            .find_map(|field| pricing.get(*field))
            .and_then(Value::as_f64)
            .filter(|price| price.is_finite() && *price >= 0.0)
            .map(|price| price * 1_000_000.0)
    };
    let input = price(&["input_price", "inputPrice"]);
    let output = price(&["output_price", "outputPrice"]);
    let cache_read = price(&["cache_read_price", "cacheReadPrice"]);
    let cache_write = price(&["cache_write_price", "cacheWritePrice"]);
    if input.is_none() && output.is_none() && cache_read.is_none() && cache_write.is_none() {
        return None;
    }
    Some(ProviderModelPrice {
        input,
        output,
        cache_read,
        cache_write,
        per_request: None,
        group: group.to_string(),
    })
}

fn classify_error(message: String) -> PricingFailure {
    if message.starts_with("HTTP 404") || message.starts_with("HTTP 501") {
        let status = message
            .split_whitespace()
            .nth(1)
            .unwrap_or("404")
            .trim_end_matches(':');
        return PricingFailure::Unsupported(format!("站点未开启模型广场（HTTP {status}）"));
    }
    if message.starts_with("HTTP 401") || message.starts_with("HTTP 403") {
        return PricingFailure::Failed("模型广场需要登录，当前凭据无法读取".to_string());
    }
    if looks_unsupported(&message) {
        PricingFailure::Unsupported(message)
    } else {
        PricingFailure::Failed(message)
    }
}

fn looks_unsupported(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("not found")
        || normalized.contains("not enabled")
        || normalized.contains("not supported")
        || normalized.contains("disabled")
        || message.contains("不存在")
        || message.contains("未开启")
        || message.contains("不支持")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_prices_are_converted_from_per_token_to_per_million() {
        let price = model_price(
            &json!({
                "name": "claude-sonnet-5",
                "pricing": {
                    "billing_mode": "token",
                    "input_price": 0.000003,
                    "output_price": 0.000015,
                    "cache_read_price": 0.0000003,
                    "cache_write_price": 0.00000375
                }
            }),
            "Claude",
        )
        .expect("token pricing should parse");
        assert_eq!(price.input, Some(3.0));
        assert_eq!(price.output, Some(15.0));
        assert_eq!(price.cache_read, Some(0.3));
        assert_eq!(price.cache_write, Some(3.75));
        assert_eq!(price.group, "Claude");
    }

    #[test]
    fn per_request_pricing_does_not_become_a_token_price() {
        let price = model_price(
            &json!({
                "name": "image-model",
                "pricing": { "billing_mode": "per_request", "per_request_price": 0.02 }
            }),
            "default",
        )
        .expect("per-request pricing should parse");
        assert_eq!(price.per_request, Some(0.02));
        assert!(!price.has_token_price());
    }

    #[test]
    fn image_billing_is_not_misrepresented_as_tokens() {
        assert!(model_price(
            &json!({
                "name": "image-model",
                "pricing": { "billing_mode": "image", "image_input_price": 0.01 }
            }),
            "default",
        )
        .is_none());
    }

    #[test]
    fn group_selection_prefers_subscription_group_then_key_group() {
        let groups = vec![
            json!({ "id": "1", "name": "default", "rate_multiplier": 1.0 }),
            json!({ "id": "2", "name": "Claude", "rate_multiplier": 1.2 }),
        ];
        assert_eq!(select_group(&groups, "Claude", "default"), Some(1));
        assert_eq!(select_group(&groups, "", "2"), Some(1));
        assert_eq!(select_group(&groups, "", "default"), Some(0));
        assert_eq!(select_group(&groups, "missing", "missing"), Some(0));
    }
}
