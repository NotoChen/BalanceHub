//! Sub2API 模型广场价目。
//!
//! `/api/v1/model-plaza` 以分组为顶层，`pricing` 是渠道单价（USD / token），
//! 实付价 = 渠道单价 × 该卡实际扣费分组的倍率（Key 分组优先，其次订阅分组），
//! 这里统一乘 1e6 转成 USD / 百万 token。按次计费保留 `per_request_price`，
//! 不与 token 单价混算。广场还会同时给出官方参考价、长上下文阶梯、分时倍率和
//! 图片分辨率档位，这些一起保存，模型广场才能说明「这个分组是打折还是溢价」。
//! 图片模式没有 token 单价时不再丢弃模型，而是按 1K / 2K / 4K 档位展示。
//! 广场受站点功能开关控制，关闭时返回 404。

use crate::{
    adapters::transport::ProviderTransport,
    limits,
    models::{
        Provider, ProviderBillingMode, ProviderGroupPricing, ProviderLongContextBasis,
        ProviderModelPrice, ProviderOfficialPrice, ProviderOptionalCapability,
        ProviderOptionalCapabilityState, ProviderPricingSnapshot, ProviderPricingSource,
        ProviderPricingTier, ProviderTimePricing, ProviderTimePricingPeriod,
    },
};
use reqwest::Method;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

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
                loaded.group_pricing,
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
    group_pricing: ProviderGroupPricing,
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
        // 200 但没有分组字段是响应结构问题，不是“站点没有价目接口”；按本次
        // 失败处理，保留上次成功读到的价格。
        .ok_or_else(|| PricingFailure::Failed("站点没有返回模型广场分组".to_string()))?;
    let key_group = current_key_group(provider);
    // API Key 所属分组才是这张卡实际扣费的分组；只有读不到 Key 分组时才去问
    // 订阅进度，避免有订阅的账号把别的分组的价目套到当前 Key 上。
    let account_group = if key_group.is_empty() {
        fetch_account_group(client, provider).await
    } else {
        String::new()
    };
    let selected = select_group(groups, &account_group, &key_group);
    let group = selected
        .and_then(|index| groups.get(index))
        .ok_or_else(|| {
            // 广场存在、只是当前凭据看不到分组：这是凭据状态，不是站点能力。
            // 绑定分组后重新探测即可恢复，旧价目继续保留。
            PricingFailure::Failed("当前账号没有可见的模型分组".to_string())
        })?;
    let group_name = string_field(group, &["name"]).unwrap_or_default();
    let rates = GroupRates::from_group(group);
    let group_pricing = group_pricing(group);

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
        let Some(price) = model_price(item, &group_name, rates) else {
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
        group_pricing,
    })
}

/// Key 分组是这张卡实际扣费的分组，优先级最高；没有 Key 分组（例如只有账号
/// 登录的卡片）时才用订阅分组，最后才退回广场里的第一个可见分组。
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
    let explicit = provider.quota.group.trim();
    if let Some(option) = provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key.trim() == current)
    {
        let group = option.group.trim();
        if !group.is_empty() {
            return group.to_string();
        }
        // 当前 Key 的条目没有分组：这是确定的信息，不能拿别的 Key 的分组
        // 来猜；只有卡片显式记录的计费分组还能作为依据。
        return explicit.to_string();
    }
    // 手工填写的 Key（不在同步列表里）没有任何归属信息。所有已分组的 Key
    // 都指向同一个分组时才能按它推断，多个分组时宁可留空。
    let groups = provider
        .auth
        .api_key_options
        .iter()
        .map(|option| option.group.trim())
        .filter(|group| !group.is_empty())
        .collect::<BTreeSet<_>>();
    if groups.len() == 1 {
        return groups.into_iter().next().unwrap_or_default().to_string();
    }
    explicit.to_string()
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
    find(key_group)
        .or_else(|| find(account_group))
        .or_else(|| (!groups.is_empty()).then_some(0))
}

/// 站点价格是渠道单价，实际扣费还要乘分组倍率；登录用户有专属倍率时以专属
/// 倍率为准。图片/视频可配置独立倍率，不随通用倍率变化。
#[derive(Debug, Clone, Copy)]
struct GroupRates {
    effective: f64,
    image: f64,
    image_independent: bool,
    video: f64,
    video_independent: bool,
}

impl Default for GroupRates {
    fn default() -> Self {
        Self {
            effective: 1.0,
            image: 1.0,
            image_independent: false,
            video: 1.0,
            video_independent: false,
        }
    }
}

impl GroupRates {
    fn from_group(group: &Value) -> Self {
        let base = amount(group, &["rate_multiplier", "rateMultiplier"]).unwrap_or(1.0);
        Self {
            effective: amount(group, &["user_rate_multiplier", "userRateMultiplier"])
                .unwrap_or(base),
            image: amount(group, &["image_rate_multiplier", "imageRateMultiplier"]).unwrap_or(1.0),
            image_independent: bool_field(
                group,
                &["image_rate_independent", "imageRateIndependent"],
            ),
            video: amount(group, &["video_rate_multiplier", "videoRateMultiplier"]).unwrap_or(1.0),
            video_independent: bool_field(
                group,
                &["video_rate_independent", "videoRateIndependent"],
            ),
        }
    }

    fn request_rate(&self, billing: ProviderBillingMode) -> f64 {
        match billing {
            ProviderBillingMode::Image if self.image_independent => self.image,
            ProviderBillingMode::Video if self.video_independent => self.video,
            _ => self.effective,
        }
    }
}

/// 分组口径只用于解释价格：价格已经按生效倍率换算，高峰倍率则在请求落入
/// 高峰时段时由站点额外叠加。
fn group_pricing(group: &Value) -> ProviderGroupPricing {
    ProviderGroupPricing {
        rate_multiplier: amount(group, &["rate_multiplier", "rateMultiplier"]),
        user_rate_multiplier: amount(group, &["user_rate_multiplier", "userRateMultiplier"]),
        peak_rate_enabled: bool_field(group, &["peak_rate_enabled", "peakRateEnabled"]),
        peak_start: string_field(group, &["peak_start", "peakStart"]).unwrap_or_default(),
        peak_end: string_field(group, &["peak_end", "peakEnd"]).unwrap_or_default(),
        peak_rate_multiplier: amount(group, &["peak_rate_multiplier", "peakRateMultiplier"]),
        long_context_pricing_enabled: group
            .get("long_context_pricing_enabled")
            .or_else(|| group.get("longContextPricingEnabled"))
            .and_then(Value::as_bool),
    }
}

fn model_price(item: &Value, group: &str, rates: GroupRates) -> Option<ProviderModelPrice> {
    let pricing = item.get("pricing").filter(|pricing| pricing.is_object())?;
    let billing_mode = string_field(pricing, &["billing_mode", "billingMode"]).unwrap_or_default();
    let billing = match billing_mode.as_str() {
        "image" => ProviderBillingMode::Image,
        "per_request" => ProviderBillingMode::PerRequest,
        "video" => ProviderBillingMode::Video,
        _ => ProviderBillingMode::Token,
    };
    let mut price = ProviderModelPrice {
        billing,
        input: token_amount(pricing, &["input_price", "inputPrice"]),
        output: token_amount(pricing, &["output_price", "outputPrice"]),
        cache_read: token_amount(pricing, &["cache_read_price", "cacheReadPrice"]),
        cache_write: token_amount(pricing, &["cache_write_price", "cacheWritePrice"]),
        per_request: amount(pricing, &["per_request_price", "perRequestPrice"]),
        group: group.to_string(),
        official: official_price(item.get("official_pricing")),
        time_pricing: time_pricing(item.get("time_pricing")),
        ..ProviderModelPrice::default()
    };
    if billing == ProviderBillingMode::Image {
        // 图片模式的 input/output 指图片 token 单价，优先于通用 token 价；
        // 1K / 2K / 4K 的按张价格在 `pricing.intervals` 里。
        price.input =
            token_amount(pricing, &["image_input_price", "imageInputPrice"]).or(price.input);
        price.output =
            token_amount(pricing, &["image_output_price", "imageOutputPrice"]).or(price.output);
    }
    price.tiers = price_tiers(pricing, &price);
    if billing == ProviderBillingMode::Token {
        // 站点只返回阶梯价时，用第一档补出基础展示价。
        if !price.has_token_price() {
            if let Some(first) = price.tiers.first() {
                price.input = first.input;
                price.output = first.output;
                price.cache_read = first.cache_read;
                price.cache_write = first.cache_write;
            }
        }
        if price.tiers.len() > 1 {
            price.long_context_basis = long_context_basis(item);
        }
    }
    apply_group_rates(&mut price, rates);
    price.has_any_price().then_some(price)
}

/// 把渠道单价换算成用户实付价。官方参考价不乘倍率，保持原样。
fn apply_group_rates(price: &mut ProviderModelPrice, rates: GroupRates) {
    let token_rate = rates.effective;
    for value in [
        &mut price.input,
        &mut price.output,
        &mut price.cache_read,
        &mut price.cache_write,
    ] {
        *value = value.map(|value| value * token_rate);
    }
    let request_rate = rates.request_rate(price.billing);
    price.per_request = price.per_request.map(|value| value * request_rate);
    for tier in &mut price.tiers {
        for value in [
            &mut tier.input,
            &mut tier.output,
            &mut tier.cache_read,
            &mut tier.cache_write,
        ] {
            *value = value.map(|value| value * token_rate);
        }
        tier.per_request = tier.per_request.map(|value| value * request_rate);
    }
}

fn bool_field(value: &Value, field_names: &[&str]) -> bool {
    field_names
        .iter()
        .any(|field| value.get(*field).and_then(Value::as_bool) == Some(true))
}

/// 官方参考价只用于对照，不参与实收价计算；缺失字段留空。
fn official_price(value: Option<&Value>) -> Option<ProviderOfficialPrice> {
    let value = value.filter(|value| value.is_object())?;
    let price = ProviderOfficialPrice {
        input: token_amount(value, &["input_price", "inputPrice"]),
        output: token_amount(value, &["output_price", "outputPrice"]),
        cache_read: token_amount(value, &["cache_read_price", "cacheReadPrice"]),
        cache_write: token_amount(value, &["cache_write_price", "cacheWritePrice"]),
    };
    price.has_price().then_some(price)
}

/// 站点的多档定价：长上下文阶梯给出每档 token 单价，图片计费给出每档按张价。
/// 管理端也可能只配倍率，差额由基础价格乘倍率补出来。
fn price_tiers(pricing: &Value, base: &ProviderModelPrice) -> Vec<ProviderPricingTier> {
    pricing
        .get("intervals")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let tier = ProviderPricingTier {
                min_tokens: u64_field(item, &["min_tokens", "minTokens"]).unwrap_or(0),
                max_tokens: u64_field(item, &["max_tokens", "maxTokens"]),
                label: string_field(item, &["tier_label", "tierLabel"]).unwrap_or_default(),
                input: tier_amount(
                    item,
                    &["input_price", "inputPrice"],
                    &["input_multiplier", "inputMultiplier"],
                    base.input,
                ),
                output: tier_amount(
                    item,
                    &["output_price", "outputPrice"],
                    &["output_multiplier", "outputMultiplier"],
                    base.output,
                ),
                cache_read: tier_amount(
                    item,
                    &["cache_read_price", "cacheReadPrice"],
                    &["cache_read_multiplier", "cacheReadMultiplier"],
                    base.cache_read,
                ),
                cache_write: tier_amount(
                    item,
                    &["cache_write_price", "cacheWritePrice"],
                    &["cache_write_multiplier", "cacheWriteMultiplier"],
                    base.cache_write,
                ),
                per_request: amount(item, &["per_request_price", "perRequestPrice"]),
            };
            tier.has_price().then_some(tier)
        })
        .collect()
}

fn tier_amount(
    item: &Value,
    price_fields: &[&str],
    multiplier_fields: &[&str],
    base: Option<f64>,
) -> Option<f64> {
    token_amount(item, price_fields).or_else(|| {
        let multiplier = amount(item, multiplier_fields)?;
        base.map(|base| base * multiplier)
    })
}

fn long_context_basis(item: &Value) -> Option<ProviderLongContextBasis> {
    match string_field(item, &["long_context_basis", "longContextBasis"]).as_deref() {
        Some("whole_request" | "wholeRequest") => Some(ProviderLongContextBasis::WholeRequest),
        Some("marginal") => Some(ProviderLongContextBasis::Marginal),
        _ => None,
    }
}

/// 分时倍率；站点只输出倍率不为 1 的时段。时区随站点配置，展示时原样带上。
fn time_pricing(value: Option<&Value>) -> Option<ProviderTimePricing> {
    let value = value.filter(|value| value.is_object())?;
    let periods = value
        .get("periods")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|period| {
            let start = string_field(period, &["start_time", "startTime"])?;
            let end = string_field(period, &["end_time", "endTime"])?;
            let multiplier = amount(period, &["multiplier"])?;
            (multiplier > 0.0 && (multiplier - 1.0).abs() > f64::EPSILON).then_some(
                ProviderTimePricingPeriod {
                    start,
                    end,
                    multiplier,
                },
            )
        })
        .collect::<Vec<_>>();
    (!periods.is_empty()).then(|| ProviderTimePricing {
        timezone: string_field(value, &["timezone"]).unwrap_or_default(),
        weekdays_only: value
            .get("weekdays_only")
            .or_else(|| value.get("weekdaysOnly"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        periods,
    })
}

fn token_amount(value: &Value, field_names: &[&str]) -> Option<f64> {
    amount(value, field_names).map(|price| price * 1_000_000.0)
}

/// 原始金额：USD / token、USD / 次或倍率，不做单位换算。
fn amount(value: &Value, field_names: &[&str]) -> Option<f64> {
    field_names
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_f64))
        .filter(|price| price.is_finite() && *price >= 0.0)
}

fn u64_field(value: &Value, field_names: &[&str]) -> Option<u64> {
    field_names.iter().find_map(|field| {
        let item = value.get(*field)?;
        item.as_u64()
            .or_else(|| item.as_i64().and_then(|number| u64::try_from(number).ok()))
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
    use crate::adapters::test_support::{serve, Reply};
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
            GroupRates::default(),
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
            GroupRates::default(),
        )
        .expect("per-request pricing should parse");
        assert_eq!(price.per_request, Some(0.02));
        assert!(!price.has_token_price());
    }

    #[test]
    fn image_billing_keeps_image_token_prices_without_pretending_they_are_text() {
        let price = model_price(
            &json!({
                "name": "image-model",
                "pricing": {
                    "billing_mode": "image",
                    "input_price": 0.00001,
                    "image_input_price": 0.000000252,
                    "image_output_price": 0.000002
                }
            }),
            "default",
            GroupRates::default(),
        )
        .expect("image pricing should parse");
        assert_eq!(price.billing, ProviderBillingMode::Image);
        assert_eq!(price.input, Some(0.252));
        assert_eq!(price.output, Some(2.0));
    }

    #[test]
    fn image_resolution_tiers_stay_per_image() {
        let price = model_price(
            &json!({
                "name": "image-model",
                "pricing": {
                    "billing_mode": "image",
                    "intervals": [
                        { "tier_label": "1K", "per_request_price": 0.04 },
                        { "tier_label": "2K", "per_request_price": 0.08 }
                    ]
                }
            }),
            "default",
            GroupRates::default(),
        )
        .expect("image tiers should parse");
        assert_eq!(price.billing, ProviderBillingMode::Image);
        assert!(!price.has_token_price());
        assert_eq!(price.tiers.len(), 2);
        assert_eq!(price.tiers[0].label, "1K");
        assert_eq!(price.tiers[0].per_request, Some(0.04));
        assert_eq!(price.tiers[1].per_request, Some(0.08));
    }

    #[test]
    fn official_price_tiers_and_time_pricing_travel_with_the_site_price() {
        let price = model_price(
            &json!({
                "name": "gpt-5",
                "pricing": {
                    "billing_mode": "token",
                    "input_price": 0.0000025,
                    "output_price": 0.00001,
                    "intervals": [
                        {
                            "min_tokens": 0,
                            "max_tokens": 272000,
                            "tier_label": "≤272K",
                            "input_price": 0.0000025,
                            "output_price": 0.00001
                        },
                        {
                            "min_tokens": 272000,
                            "tier_label": ">272K",
                            "input_multiplier": 2.0,
                            "output_multiplier": 2.0
                        }
                    ]
                },
                "official_pricing": {
                    "input_price": 0.000005,
                    "output_price": 0.000015,
                    "cache_read_price": 0.0000005
                },
                "long_context_basis": "whole_request",
                "time_pricing": {
                    "timezone": "Asia/Shanghai",
                    "weekdays_only": true,
                    "periods": [
                        { "start_time": "00:30", "end_time": "08:30", "multiplier": 0.5 },
                        { "start_time": "12:00", "end_time": "13:00", "multiplier": 1.0 }
                    ]
                }
            }),
            "Claude",
            GroupRates::default(),
        )
        .expect("rich token pricing should parse");
        let official = price.official.expect("official price should be kept");
        assert_eq!(official.input, Some(5.0));
        assert_eq!(official.output, Some(15.0));
        assert_eq!(official.cache_read, Some(0.5));
        assert_eq!(price.tiers.len(), 2);
        assert_eq!(price.tiers[1].label, ">272K");
        assert_eq!(price.tiers[1].input, Some(5.0));
        assert_eq!(price.tiers[1].output, Some(20.0));
        assert_eq!(
            price.long_context_basis,
            Some(ProviderLongContextBasis::WholeRequest)
        );
        let time = price.time_pricing.expect("time pricing should be kept");
        assert_eq!(time.timezone, "Asia/Shanghai");
        assert!(time.weekdays_only);
        assert_eq!(time.periods.len(), 1, "倍率为 1 的时段不应产生分时提示");
        assert_eq!(time.periods[0].start, "00:30");
        assert_eq!(time.periods[0].multiplier, 0.5);
    }

    #[test]
    fn user_specific_rate_is_applied_to_the_paid_price_but_not_to_the_official_price() {
        let price = model_price(
            &json!({
                "name": "gpt-5",
                "pricing": {
                    "billing_mode": "token",
                    "input_price": 0.0000025,
                    "output_price": 0.00001
                },
                "official_pricing": {
                    "input_price": 0.000005,
                    "output_price": 0.000015
                }
            }),
            "vip",
            GroupRates::from_group(&json!({
                "rate_multiplier": 1.2,
                "user_rate_multiplier": 0.8
            })),
        )
        .expect("token pricing should parse");
        assert_eq!(price.input, Some(2.0));
        assert_eq!(price.output, Some(8.0));
        assert_eq!(
            price.official.as_ref().and_then(|official| official.input),
            Some(5.0),
            "官方参考价只用于对比，不应乘站点倍率"
        );
    }

    #[test]
    fn image_independent_rate_overrides_the_group_rate_for_per_image_prices() {
        let price = model_price(
            &json!({
                "name": "image-model",
                "pricing": {
                    "billing_mode": "image",
                    "intervals": [{ "tier_label": "1K", "per_request_price": 0.04 }]
                }
            }),
            "media",
            GroupRates::from_group(&json!({
                "rate_multiplier": 1.5,
                "image_rate_independent": true,
                "image_rate_multiplier": 0.5
            })),
        )
        .expect("image pricing should parse");
        assert_eq!(price.tiers[0].per_request, Some(0.02));
    }

    #[test]
    fn video_billing_uses_its_independent_rate_and_stays_per_request() {
        let price = model_price(
            &json!({
                "name": "video-model",
                "pricing": {
                    "billing_mode": "video",
                    "per_request_price": 0.5
                }
            }),
            "media",
            GroupRates::from_group(&json!({
                "rate_multiplier": 1.5,
                "video_rate_independent": true,
                "video_rate_multiplier": 0.7
            })),
        )
        .expect("video pricing should parse");
        assert_eq!(price.billing, ProviderBillingMode::Video);
        assert_eq!(price.per_request, Some(0.35));
        assert!(!price.has_token_price());
    }

    #[test]
    fn group_pricing_records_peak_and_user_rate_for_the_price_note() {
        let pricing = group_pricing(&json!({
            "rate_multiplier": 1.2,
            "user_rate_multiplier": 0.8,
            "peak_rate_enabled": true,
            "peak_start": "09:00",
            "peak_end": "18:00",
            "peak_rate_multiplier": 1.5,
            "long_context_pricing_enabled": false
        }));
        assert_eq!(pricing.rate_multiplier, Some(1.2));
        assert_eq!(pricing.user_rate_multiplier, Some(0.8));
        assert!(pricing.peak_rate_enabled);
        assert_eq!(pricing.peak_start, "09:00");
        assert_eq!(pricing.peak_end, "18:00");
        assert_eq!(pricing.peak_rate_multiplier, Some(1.5));
        assert_eq!(pricing.long_context_pricing_enabled, Some(false));
    }

    #[test]
    fn group_selection_prefers_the_key_group_over_the_subscription_group() {
        let groups = vec![
            json!({ "id": "1", "name": "default", "rate_multiplier": 1.0 }),
            json!({ "id": "2", "name": "Claude", "rate_multiplier": 1.2 }),
        ];
        // 有订阅但 Key 绑在别的分组时，按 Key 分组计价才是实际扣费口径。
        assert_eq!(select_group(&groups, "Claude", "default"), Some(0));
        assert_eq!(select_group(&groups, "missing", "Claude"), Some(1));
        // 没有 Key 分组（只有账号登录）时才退回订阅分组。
        assert_eq!(select_group(&groups, "Claude", ""), Some(1));
        assert_eq!(select_group(&groups, "", "2"), Some(1));
        assert_eq!(select_group(&groups, "", "default"), Some(0));
        assert_eq!(select_group(&groups, "missing", "missing"), Some(0));
    }

    fn sub2_provider(keys: &[(&str, &str)], current: &str) -> Provider {
        let mut input = crate::models::ProviderInput::default();
        input.identity.protocol = crate::models::ProviderProtocol::Sub2Api;
        input.identity.base_url = "https://relay.example.invalid".into();
        input.auth.mode = crate::models::AuthMode::ApiKey;
        let mut provider = Provider::from_input(input, "sub2-pricing-fixture".into());
        provider.auth.api_key = current.to_string();
        provider.auth.api_key_options = keys
            .iter()
            .map(|(key, group)| {
                let mut option = crate::models::ProviderApiKeyOption::current_for_protocol(
                    key,
                    crate::models::ProviderProtocol::Sub2Api,
                );
                option.group = (*group).to_string();
                option
            })
            .collect();
        provider
    }

    #[test]
    fn key_group_follows_the_current_key_instead_of_the_first_grouped_key() {
        let provider = sub2_provider(&[("sk-a", "Claude"), ("sk-b", "Codex")], "sk-b");
        assert_eq!(current_key_group(&provider), "Codex");
    }

    #[test]
    fn current_key_without_a_group_does_not_borrow_another_keys_group() {
        let mut provider = sub2_provider(&[("sk-a", "Claude"), ("sk-b", "")], "sk-b");
        assert_eq!(current_key_group(&provider), "");
        provider.quota.group = "explicit".into();
        assert_eq!(current_key_group(&provider), "explicit");
    }

    #[test]
    fn manually_entered_key_only_infers_a_group_when_all_keys_share_one() {
        let single = sub2_provider(&[("sk-a", "Codex"), ("sk-b", "Codex")], "sk-manual");
        assert_eq!(current_key_group(&single), "Codex");
        let mixed = sub2_provider(&[("sk-a", "Codex"), ("sk-b", "Claude")], "sk-manual");
        assert_eq!(current_key_group(&mixed), "");
        assert_eq!(current_key_group(&sub2_provider(&[], "sk-manual")), "");
    }

    fn priced_provider() -> Provider {
        let mut provider = sub2_provider(&[("sk-a", "Codex")], "sk-a");
        provider.capabilities.pricing.mark_loaded(
            ProviderPricingSource::Sub2Api,
            BTreeMap::from([("gpt-5".to_string(), ProviderModelPrice::default())]),
            BTreeMap::new(),
            "Codex".to_string(),
            ProviderGroupPricing::default(),
        );
        provider
    }

    async fn fixture_client(provider: &Provider) -> ProviderTransport {
        crate::adapters::transport::build_client(&crate::models::AppSettings::default(), provider)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_empty_plaza_keeps_the_last_price_snapshot_and_stays_unknown() {
        // 广场存在，只是当前凭据没有可见分组：不能记成站点不支持价目。
        let (provider, _requests, server) = serve(vec![Reply(200, r#"{"groups":[]}"#)]);
        let provider = {
            let mut provider = provider;
            provider.capabilities.pricing = priced_provider().capabilities.pricing;
            provider
        };
        let client = fixture_client(&provider).await;
        let probe = probe_pricing(&client, &provider).await;
        server.join().unwrap();
        assert!(!probe.state.known);
        assert!(probe
            .state
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有可见的模型分组"));
        assert_eq!(probe.snapshot.source, ProviderPricingSource::Sub2Api);
        assert_eq!(probe.snapshot.models.len(), 1);
    }

    #[tokio::test]
    async fn an_unexpected_plaza_payload_is_a_failed_check_not_an_unsupported_site() {
        let (provider, _requests, server) = serve(vec![Reply(200, r#"{"data":[]}"#)]);
        let provider = {
            let mut provider = provider;
            provider.capabilities.pricing = priced_provider().capabilities.pricing;
            provider
        };
        let client = fixture_client(&provider).await;
        let probe = probe_pricing(&client, &provider).await;
        server.join().unwrap();
        assert!(!probe.state.known);
        assert!(probe
            .state
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有返回模型广场分组"));
        assert_eq!(probe.snapshot.source, ProviderPricingSource::Sub2Api);
    }
}
