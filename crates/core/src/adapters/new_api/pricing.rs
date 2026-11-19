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
        Provider, ProviderGroupPricing, ProviderModelPrice, ProviderOptionalCapability,
        ProviderOptionalCapabilityState, ProviderPricingSnapshot, ProviderPricingSource,
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
    let decoded = serde_json::from_str::<Value>(&body);
    // NewAPI 把“价目模块未开启”放在标准错误包里返回，状态码可能是 403。
    // 明确未开启属于能力结论；认证或权限错误仍只记为本次失败，不能用一次
    // 拒绝清空上次成功读到的价目。
    if let Ok(value) = &decoded {
        if value.get("success").and_then(Value::as_bool) == Some(false) {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("接口返回失败")
                .trim()
                .to_string();
            return Err(if looks_unsupported(&message) {
                PricingFailure::Unsupported(message)
            } else if status.is_success() {
                PricingFailure::Failed(message)
            } else {
                PricingFailure::Failed(format!("HTTP {}: {message}", status.as_u16()))
            });
        }
    }
    if !status.is_success() {
        return Err(PricingFailure::Failed(format!(
            "HTTP {}: {}",
            status.as_u16(),
            trim_message(&body)
        )));
    }
    let decoded =
        decoded.map_err(|error| PricingFailure::Failed(format!("解析站点价目失败: {error}")))?;
    if decoded.get("success").and_then(Value::as_bool) != Some(true) {
        let message = decoded
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("接口返回失败")
            .trim()
            .to_string();
        return Err(if looks_unsupported(&message) {
            PricingFailure::Unsupported(message)
        } else {
            PricingFailure::Failed(message)
        });
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
        group: group.clone(),
        group_pricing: ProviderGroupPricing {
            rate_multiplier: Some(ratio),
            ..ProviderGroupPricing::default()
        },
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

/// NewAPI 计费分组：令牌自己指定了分组时以令牌分组为准，只有令牌没写分组
/// （或写的是无法静态定价的 `auto`）时才用账号分组。`auto` 分组由站点在请求
/// 时挑选，价目接口拿不到确定倍率，按账号分组估算。
fn effective_group(
    provider: &Provider,
    groups: &BTreeMap<String, f64>,
    usable_groups: &[String],
) -> String {
    let current = provider.auth.api_key.trim();
    let current_key_group = provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key.trim() == current)
        .map(|option| option.group.trim().to_string())
        .unwrap_or_default();
    // 手工填写的 Key 不在列表里；只有所有已分组 Key 都指向同一分组时才按它推断。
    let key_groups = provider
        .auth
        .api_key_options
        .iter()
        .map(|option| option.group.trim())
        .filter(|group| !group.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let inferred_key_group = if key_groups.len() == 1 {
        key_groups
            .into_iter()
            .next()
            .unwrap_or_default()
            .to_string()
    } else {
        String::new()
    };
    let candidates = [
        current_key_group,
        provider.quota.group.trim().to_string(),
        inferred_key_group,
    ];
    for candidate in candidates {
        if candidate.is_empty() || candidate == "auto" {
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
    let price = if quota_type == 1 {
        let per_request = extract_f64_field(item, &["model_price", "modelPrice"])
            .filter(|price| price.is_finite() && *price >= 0.0)
            .map(|price| price * group_ratio)?;
        ProviderModelPrice {
            per_request: Some(per_request),
            group: group.to_string(),
            ..ProviderModelPrice::default()
        }
    } else {
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
        let create_cache_ratio =
            extract_f64_field(item, &["create_cache_ratio", "createCacheRatio"])
                .filter(|ratio| ratio.is_finite() && *ratio >= 0.0);
        ProviderModelPrice {
            input: Some(input),
            output: Some(input * completion_ratio),
            cache_read: cache_ratio.map(|ratio| input * ratio),
            cache_write: create_cache_ratio.map(|ratio| input * ratio),
            per_request: None,
            group: group.to_string(),
            ..ProviderModelPrice::default()
        }
    };
    Some(with_catalog_metadata(price, item))
}

/// NewAPI 的价目响应同时携带模型目录信息。这些字段不是价格，但对模型广场
/// 很重要：新模型可能尚未进入 models.dev，站点自己声明的上下文和能力才是
/// 用户实际能用的规格。
fn with_catalog_metadata(mut price: ProviderModelPrice, item: &Value) -> ProviderModelPrice {
    price.context_window =
        extract_u64_field(item, &["context_length", "contextLength"]).filter(|value| *value > 0);
    price.max_output_tokens = extract_u64_field(item, &["max_output_tokens", "maxOutputTokens"])
        .filter(|value| *value > 0);
    price.input_modalities = string_list_field(item, &["input_modalities", "inputModalities"]);
    price.output_modalities = string_list_field(item, &["output_modalities", "outputModalities"]);
    price.capabilities = string_list_field(item, &["capabilities"]);
    price
}

fn extract_u64_field(value: &Value, field_names: &[&str]) -> Option<u64> {
    field_names.iter().find_map(|field_name| {
        let item = value.get(*field_name)?;
        item.as_u64().or_else(|| {
            item.as_str()
                .and_then(|text| text.trim().parse::<u64>().ok())
        })
    })
}

fn string_list_field(value: &Value, field_names: &[&str]) -> Vec<String> {
    field_names
        .iter()
        .find_map(|field_name| value.get(*field_name))
        .map(|item| match item {
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
            Value::String(text) => text
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
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
    use crate::adapters::test_support::{serve, Reply};
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
    fn catalog_metadata_travels_with_the_site_price() {
        let price = model_price(
            &json!({
                "model_name": "vision-model",
                "model_ratio": 1.0,
                "context_length": 200000,
                "max_output_tokens": 8192,
                "input_modalities": ["text", "image"],
                "output_modalities": ["text"],
                "capabilities": ["function_calling", "vision"]
            }),
            1.0,
            500_000.0,
            "default",
        )
        .expect("catalog model should have a price");
        assert_eq!(price.context_window, Some(200_000));
        assert_eq!(price.max_output_tokens, Some(8192));
        assert_eq!(price.input_modalities, ["text", "image"]);
        assert_eq!(price.output_modalities, ["text"]);
        assert_eq!(price.capabilities, ["function_calling", "vision"]);
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

    fn pricing_provider(current: &str, account_group: &str, keys: &[(&str, &str)]) -> Provider {
        let mut provider = Provider::from_input(Default::default(), "p1".into());
        provider.auth.api_key = current.to_string();
        provider.quota.group = account_group.to_string();
        provider.auth.api_key_options = keys
            .iter()
            .map(|(key, group)| {
                let mut option = crate::models::ProviderApiKeyOption::current_for_protocol(
                    key,
                    crate::models::ProviderProtocol::NewApi,
                );
                option.key = (*key).to_string();
                option.group = (*group).to_string();
                option
            })
            .collect();
        provider
    }

    #[test]
    fn effective_group_prefers_the_current_keys_group_over_the_account_group() {
        let groups = BTreeMap::from([
            ("default".to_string(), 1.0),
            ("vip".to_string(), 1.5),
            ("svip".to_string(), 2.0),
        ]);
        let usable = ["default".to_string(), "vip".to_string(), "svip".to_string()];
        // 令牌指定了分组：站点按令牌分组计费，即使账号本身属于另一个分组。
        let keyed = pricing_provider("sk-a", "vip", &[("sk-a", "svip"), ("sk-b", "default")]);
        assert_eq!(effective_group(&keyed, &groups, &usable), "svip");
        // 令牌没写分组：退回账号分组。
        let groupless = pricing_provider("sk-a", "vip", &[("sk-a", "")]);
        assert_eq!(effective_group(&groupless, &groups, &usable), "vip");
        // auto 分组无法静态定价，按账号分组估算。
        let auto = pricing_provider("sk-a", "vip", &[("sk-a", "auto")]);
        assert_eq!(effective_group(&auto, &groups, &usable), "vip");
        // 手工填写的 Key 不在列表里：所有 Key 同属一组才按它推断。
        let manual = pricing_provider("sk-manual", "vip", &[("sk-a", "svip"), ("sk-b", "svip")]);
        assert_eq!(effective_group(&manual, &groups, &usable), "vip");
        let mixed = pricing_provider("sk-manual", "vip", &[("sk-a", "svip"), ("sk-b", "default")]);
        assert_eq!(effective_group(&mixed, &groups, &usable), "vip");
    }

    #[test]
    fn effective_group_falls_back_to_default_or_the_first_group() {
        let groups = BTreeMap::from([("default".to_string(), 1.0), ("vip".to_string(), 1.5)]);
        let provider = pricing_provider("sk-a", "", &[]);
        assert_eq!(effective_group(&provider, &groups, &[]), "default");
        let only_vip = BTreeMap::from([("vip".to_string(), 1.5)]);
        assert_eq!(effective_group(&provider, &only_vip, &[]), "vip");
    }

    const STATUS_BODY: &str =
        r#"{"success":true,"system_name":"fixture","data":{"quota_per_unit":500000}}"#;
    const PRICING_DISABLED: &str = r#"{"success":false,"message":"pricing is disabled"}"#;
    const PRICING_UNAUTHORIZED: &str = r#"{"success":false,"message":"无权进行此操作"}"#;

    async fn fixture_client(provider: &Provider) -> ProviderTransport {
        crate::adapters::transport::build_client(&crate::models::AppSettings::default(), provider)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn disabled_pricing_module_is_a_capability_conclusion_even_with_http_403() {
        // 状态、带凭据的价目请求、匿名重试。
        let (provider, _requests, server) = serve(vec![
            Reply(200, STATUS_BODY),
            Reply(403, PRICING_DISABLED),
            Reply(403, PRICING_DISABLED),
        ]);
        let client = fixture_client(&provider).await;
        let probe = probe_pricing(&client, &provider).await;
        server.join().unwrap();
        assert!(probe.state.known);
        assert!(!probe.state.supported);
        assert!(probe
            .state
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("pricing is disabled"));
        assert_eq!(probe.snapshot.source, ProviderPricingSource::None);
    }

    #[tokio::test]
    async fn permission_errors_keep_the_last_successful_price_snapshot() {
        let (mut provider, _requests, server) = serve(vec![
            Reply(200, STATUS_BODY),
            Reply(403, PRICING_UNAUTHORIZED),
            Reply(403, PRICING_UNAUTHORIZED),
        ]);
        provider.capabilities.pricing.mark_loaded(
            ProviderPricingSource::NewApi,
            BTreeMap::from([("gpt-5".to_string(), ProviderModelPrice::default())]),
            BTreeMap::new(),
            "vip".to_string(),
            ProviderGroupPricing::default(),
        );
        let client = fixture_client(&provider).await;
        let probe = probe_pricing(&client, &provider).await;
        server.join().unwrap();
        assert!(!probe.state.known);
        assert_eq!(probe.snapshot.source, ProviderPricingSource::NewApi);
        assert!(probe
            .snapshot
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("无权进行此操作"));
    }
}
