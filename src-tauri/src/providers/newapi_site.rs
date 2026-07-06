use crate::models::{Provider, ProviderQuotaDisplay};
use crate::util::DEFAULT_QUOTA_PER_UNIT;
use reqwest::{
    header::{ACCEPT, COOKIE, ORIGIN, REFERER, USER_AGENT},
    Client, Url,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::newapi_http::{build_url, USER_AGENT_VALUE};
use super::newapi_response::{
    cloudflare_challenge_message, extract_f64_field, extract_string_field, is_cloudflare_challenge,
    trim_message,
};

#[derive(Debug, Clone)]
pub struct SiteMetadata {
    pub system_name: String,
    pub logo: String,
    pub quota_per_unit: f64,
    pub quota_display_type: String,
    pub currency_symbol: String,
    pub currency_exchange_rate: f64,
}

impl Default for SiteMetadata {
    fn default() -> Self {
        Self {
            system_name: String::new(),
            logo: String::new(),
            quota_per_unit: DEFAULT_QUOTA_PER_UNIT,
            quota_display_type: "currency".to_string(),
            currency_symbol: "$".to_string(),
            currency_exchange_rate: 1.0,
        }
    }
}

const SITE_METADATA_TTL: Duration = Duration::from_secs(300);

fn site_metadata_cache() -> &'static Mutex<HashMap<String, (Instant, SiteMetadata)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, SiteMetadata)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 站点元数据（货币符号/额度单位等）基本不变，缓存 5 分钟，
/// 避免每次刷新、用量趋势、连接测试都重复探测同一站点的 `/api/status`。
pub(crate) async fn fetch_site_metadata(
    client: &Client,
    base_url: &str,
    is_anyrouter: bool,
) -> Result<SiteMetadata, String> {
    let cache_key = format!("{is_anyrouter}|{base_url}");
    if let Some((fetched_at, metadata)) = site_metadata_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        if fetched_at.elapsed() < SITE_METADATA_TTL {
            return Ok(metadata);
        }
    }

    let metadata = fetch_site_metadata_uncached(client, base_url, is_anyrouter).await?;
    if let Ok(mut cache) = site_metadata_cache().lock() {
        cache.insert(cache_key, (Instant::now(), metadata.clone()));
    }
    Ok(metadata)
}

async fn fetch_site_metadata_uncached(
    client: &Client,
    base_url: &str,
    is_anyrouter: bool,
) -> Result<SiteMetadata, String> {
    let url = build_url(base_url, "/api/status")?;
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"));

    if is_anyrouter {
        let cookie_header = super::anyrouter::challenge_cookie_header(client, base_url).await?;
        if !cookie_header.trim().is_empty() {
            request = request.header(COOKIE, cookie_header);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("请求系统状态失败: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("读取系统状态失败: {err}"))?;

    if !status.is_success() {
        if is_cloudflare_challenge(&body) {
            return Err(cloudflare_challenge_message());
        }
        return Err(format!("HTTP {}: {}", status.as_u16(), trim_message(&body)));
    }

    if is_cloudflare_challenge(&body) {
        return Err(cloudflare_challenge_message());
    }

    if body.contains("var arg1") {
        return Err("命中 AnyRouter 验证页，动态 Cookie 未通过".to_string());
    }

    let decoded = serde_json::from_str::<Value>(&body)
        .map_err(|err| format!("解析系统状态失败: {err}: {}", trim_message(&body)))?;

    Ok(extract_site_metadata(&decoded, base_url))
}

pub(crate) fn extract_site_metadata(value: &Value, base_url: &str) -> SiteMetadata {
    let mut metadata = SiteMetadata {
        system_name: extract_system_name(value).unwrap_or_default(),
        logo: extract_logo(value, base_url).unwrap_or_default(),
        ..SiteMetadata::default()
    };
    metadata.quota_per_unit = extract_f64_field(
        value.pointer("/data").unwrap_or(value),
        &["quota_per_unit", "quotaPerUnit"],
    )
    .filter(|value| *value > 0.0)
    .unwrap_or(metadata.quota_per_unit);
    let quota_display_type = extract_string_field(
        value.pointer("/data").unwrap_or(value),
        &["quota_display_type", "quotaDisplayType"],
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
    .unwrap_or(metadata.quota_display_type);
    metadata.quota_display_type = normalize_quota_display_type(&quota_display_type);
    let currency_symbol = extract_string_field(
        value.pointer("/data").unwrap_or(value),
        &["custom_currency_symbol", "customCurrencySymbol"],
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
    metadata.currency_symbol = normalize_currency_symbol(&quota_display_type, currency_symbol);
    metadata.currency_exchange_rate = extract_f64_field(
        value.pointer("/data").unwrap_or(value),
        &[
            "custom_currency_exchange_rate",
            "customCurrencyExchangeRate",
        ],
    )
    .filter(|value| *value > 0.0)
    .unwrap_or(metadata.currency_exchange_rate);
    metadata
}

pub(crate) fn site_metadata_from_provider(provider: &Provider) -> SiteMetadata {
    let quota_display_type = if provider.quota.display_type.trim().is_empty() {
        "currency".to_string()
    } else {
        normalize_quota_display_type(&provider.quota.display_type)
    };
    SiteMetadata {
        system_name: provider.identity.name.clone(),
        logo: provider.identity.site_logo.clone(),
        quota_per_unit: if provider.quota.per_unit > 0.0 {
            provider.quota.per_unit
        } else {
            DEFAULT_QUOTA_PER_UNIT
        },
        quota_display_type,
        currency_symbol: normalize_currency_symbol(
            &provider.quota.display_type,
            Some(provider.quota.currency_symbol.clone()),
        ),
        currency_exchange_rate: if provider.quota.currency_exchange_rate > 0.0 {
            provider.quota.currency_exchange_rate
        } else {
            1.0
        },
    }
}

pub(crate) fn apply_site_metadata(provider: &mut Provider, site: SiteMetadata) {
    if !site.system_name.trim().is_empty() {
        provider.identity.name = site.system_name.clone();
    }
    provider.identity.site_logo = site.logo;
    provider.quota.per_unit = site.quota_per_unit;
    provider.quota.display_type = site.quota_display_type;
    provider.quota.currency_symbol = site.currency_symbol;
    provider.quota.currency_exchange_rate = site.currency_exchange_rate;
}

pub(crate) fn convert_quota_value(value: i64, site: &SiteMetadata) -> (f64, ProviderQuotaDisplay) {
    let display_type = site.quota_display_type.trim();
    if display_type.eq_ignore_ascii_case("tokens") {
        return (
            value as f64,
            ProviderQuotaDisplay {
                quota_display_type: "tokens".to_string(),
                currency_symbol: String::new(),
            },
        );
    }

    let unit = if site.quota_per_unit > 0.0 {
        site.quota_per_unit
    } else {
        DEFAULT_QUOTA_PER_UNIT
    };
    let exchange_rate = if site.currency_exchange_rate > 0.0 {
        site.currency_exchange_rate
    } else {
        1.0
    };
    (
        value as f64 / unit * exchange_rate,
        ProviderQuotaDisplay {
            quota_display_type: "currency".to_string(),
            currency_symbol: if site.currency_symbol.trim().is_empty() {
                "$".to_string()
            } else {
                site.currency_symbol.clone()
            },
        },
    )
}

pub(crate) fn value_to_string(value: Option<Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn extract_system_name(value: &Value) -> Option<String> {
    value
        .pointer("/data/system_name")
        .or_else(|| value.pointer("/data/systemName"))
        .or_else(|| value.get("system_name"))
        .or_else(|| value.get("systemName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
}

fn extract_logo(value: &Value, base_url: &str) -> Option<String> {
    let logo = value
        .pointer("/data/logo")
        .or_else(|| value.get("logo"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|logo| !logo.is_empty())?;

    if logo.starts_with("http://") || logo.starts_with("https://") || logo.starts_with("data:") {
        return Some(logo.to_string());
    }

    Url::parse(base_url)
        .ok()
        .and_then(|base| base.join(logo).ok())
        .map(|url| url.to_string())
}

fn normalize_quota_display_type(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("tokens") {
        "tokens".to_string()
    } else {
        "currency".to_string()
    }
}

fn normalize_currency_symbol(display_type: &str, symbol: Option<String>) -> String {
    let display_type = display_type.trim();
    if display_type.eq_ignore_ascii_case("tokens") {
        return String::new();
    }

    if let Some(symbol) = known_currency_symbol(display_type) {
        return symbol.to_string();
    }

    let symbol = symbol.unwrap_or_default();
    let symbol = symbol.trim();
    if !symbol.is_empty() && symbol != "¤" {
        return symbol.to_string();
    }

    "$".to_string()
}

fn known_currency_symbol(display_type: &str) -> Option<&'static str> {
    match display_type.trim().to_ascii_uppercase().as_str() {
        "USD" | "US_DOLLAR" | "US DOLLAR" => Some("$"),
        "CNY" | "RMB" | "CNH" | "YUAN" | "人民币" => Some("¥"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usd_display_type_takes_priority_over_custom_symbol() {
        let metadata = extract_site_metadata(
            &json!({
                "data": {
                    "quota_display_type": "USD",
                    "quota_per_unit": 500000,
                    "custom_currency_symbol": "Ɇ ",
                    "custom_currency_exchange_rate": 1
                }
            }),
            "https://elysiver.h-e.top",
        );

        assert_eq!(metadata.quota_display_type, "currency");
        assert_eq!(metadata.quota_per_unit, 500000.0);
        assert_eq!(metadata.currency_symbol, "$");
    }

    #[test]
    fn custom_symbol_is_kept_for_unknown_currency_display_type() {
        let metadata = extract_site_metadata(
            &json!({
                "data": {
                    "quota_display_type": "custom",
                    "custom_currency_symbol": "点"
                }
            }),
            "https://example.com",
        );

        assert_eq!(metadata.quota_display_type, "currency");
        assert_eq!(metadata.currency_symbol, "点");
    }
}
