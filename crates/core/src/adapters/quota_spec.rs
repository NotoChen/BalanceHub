//! 通用 API Key 中转站的余额声明求值。
//!
//! 请求走 `ProviderTransport`，因此代理、备用地址、过盾和自定义请求头与其它
//! 协议查询完全一致；这里只负责取 JSON、按表达式换算并写回 `ProviderQuota`。

use reqwest::header::RETRY_AFTER;
use reqwest::RequestBuilder;
use serde_json::Value;

use crate::{
    adapters::{error::AdapterError, transport::ProviderTransport},
    models::{
        Provider, ProviderAuthScheme, ProviderHeader, ProviderQuota, ProviderQuotaScope,
        ProviderQuotaWindow,
    },
    quota_expression,
};

const QUOTA_TIMEOUT_SECS: u64 = 30;

pub async fn fetch_quota(
    transport: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderQuota, AdapterError> {
    let spec = &provider.quota_spec;
    if !spec.is_configured() {
        return Err(AdapterError::transient("尚未配置余额接口"));
    }
    let url = resolve_provider_url(&provider.identity.base_url, &spec.url)?;
    let mut request = match spec.method {
        crate::models::ProviderQuotaMethod::Get => transport.get(url),
        crate::models::ProviderQuotaMethod::Post => {
            transport.post(url).json(&serde_json::json!({}))
        }
    };
    request = apply_spec_headers(request, &spec.headers);
    let token = spec.token.trim();
    if !token.is_empty() {
        request = request.bearer_auth(token);
    } else if !provider.auth.api_key.trim().is_empty() {
        request = apply_auth_scheme(
            request,
            provider.request.auth_scheme,
            &provider.auth.api_key,
        );
    }
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(QUOTA_TIMEOUT_SECS),
        transport.send(request, "读取余额"),
    )
    .await
    .map_err(|_| AdapterError::transient(format!("读取余额超时（{QUOTA_TIMEOUT_SECS} 秒）")))??;
    if !response.status.is_success() {
        let detail = response.body.chars().take(160).collect::<String>();
        return Err(AdapterError::from_status(
            format!("余额接口返回 HTTP {} {detail}", response.status),
            response.status,
            response.headers.get(RETRY_AFTER),
        ));
    }
    let root = serde_json::from_str::<Value>(&response.body)
        .map_err(|error| AdapterError::transient(format!("解析余额响应失败: {error}")))?;
    evaluate_spec(provider, &root).map_err(AdapterError::transient)
}

fn evaluate_spec(provider: &Provider, root: &Value) -> Result<ProviderQuota, String> {
    let spec = &provider.quota_spec;
    // 手写声明直接返回展示单位更符合直觉；只有显式填写 per_unit 时才换算。
    let unit = if spec.per_unit > 0.0 {
        spec.per_unit
    } else {
        1.0
    };
    let mut quota = ProviderQuota {
        available: 0.0,
        used: 0.0,
        known: false,
        total_known: false,
        scope: ProviderQuotaScope::Token,
        unlimited: false,
        per_unit: unit,
        display_type: "currency".to_string(),
        currency_symbol: if spec.currency_symbol.is_empty() {
            provider.quota.currency_symbol.clone()
        } else {
            spec.currency_symbol.clone()
        },
        currency_exchange_rate: provider.quota.currency_exchange_rate,
        windows: Vec::new(),
    };
    let mut errors = Vec::new();

    if !spec.balance.trim().is_empty() {
        match quota_expression::evaluate(&spec.balance, root) {
            Ok(balance) => {
                // -1 是常见的不限量占位值，与 NewAPI 钱包保持一致。
                if balance < 0.0 {
                    quota.unlimited = true;
                    quota.known = true;
                } else {
                    quota.available = balance / unit;
                    quota.known = true;
                    if !spec.total.trim().is_empty() {
                        match quota_expression::evaluate(&spec.total, root) {
                            Ok(total) => {
                                quota.total_known = true;
                                quota.used = ((total - balance) / unit).max(0.0);
                            }
                            Err(error) => errors.push(format!("总额表达式：{error}")),
                        }
                    }
                }
            }
            Err(error) => errors.push(format!("余额表达式：{error}")),
        }
    }

    for window in &spec.windows {
        if window.used.trim().is_empty() && window.limit.trim().is_empty() {
            continue;
        }
        let used =
            window.used.trim().is_empty().then_some(0.0).or_else(
                || match quota_expression::evaluate(&window.used, root) {
                    Ok(value) => Some(value / unit),
                    Err(error) => {
                        errors.push(format!("{}：{error}", window.label));
                        None
                    }
                },
            );
        let limit = window.limit.trim().is_empty().then_some(0.0).or_else(|| {
            match quota_expression::evaluate(&window.limit, root) {
                Ok(value) => Some(value / unit),
                Err(error) => {
                    errors.push(format!("{}：{error}", window.label));
                    None
                }
            }
        });
        let resets_at = if window.resets_at.trim().is_empty() {
            None
        } else {
            match quota_expression::evaluate_timestamp(&window.resets_at, root) {
                Ok(timestamp) => Some(timestamp),
                Err(error) => {
                    errors.push(format!("{}：{error}", window.label));
                    None
                }
            }
        };
        let (Some(used), Some(limit)) = (used, limit) else {
            continue;
        };
        quota.windows.push(ProviderQuotaWindow {
            label: window.label.clone(),
            used: used.max(0.0),
            limit: limit.max(0.0),
            resets_at,
            history: Vec::new(),
        });
        quota.known = true;
    }

    quota.windows.truncate(crate::limits::MAX_QUOTA_WINDOWS);
    if quota.known {
        Ok(quota)
    } else if errors.is_empty() {
        Err("余额表达式没有取到任何值".to_string())
    } else {
        Err(errors.join("；"))
    }
}

pub fn resolve_provider_url(base_url: &str, spec_url: &str) -> Result<String, String> {
    let spec_url = spec_url.trim();
    if spec_url.starts_with("http://") || spec_url.starts_with("https://") {
        return Ok(spec_url.to_string());
    }
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("缺少中转站地址，无法解析余额接口".to_string());
    }
    if spec_url.is_empty() {
        return Err("余额接口地址为空".to_string());
    }
    if spec_url.starts_with('/') {
        Ok(format!("{base}{spec_url}"))
    } else {
        Ok(format!("{base}/{spec_url}"))
    }
}

fn apply_spec_headers(mut request: RequestBuilder, headers: &[ProviderHeader]) -> RequestBuilder {
    for header in headers {
        let name = header.name.trim();
        if name.is_empty() {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(header.value.trim()),
        ) {
            request = request.header(name, value);
        }
    }
    request
}

pub fn apply_auth_scheme(
    request: RequestBuilder,
    scheme: ProviderAuthScheme,
    api_key: &str,
) -> RequestBuilder {
    match scheme {
        ProviderAuthScheme::Bearer => request.bearer_auth(api_key.trim()),
        ProviderAuthScheme::XApiKey => request.header("X-Api-Key", api_key.trim()),
        ProviderAuthScheme::None => request,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderProtocol};
    use serde_json::json;

    fn provider() -> Provider {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://relay.example.com".to_string();
        input.identity.protocol = ProviderProtocol::Api;
        input.quota_spec.url = "/api/user/balance".to_string();
        input.quota_spec.balance = "data.remain".to_string();
        input.quota_spec.total = "data.total".to_string();
        Provider::from_input(input, "quota-test".to_string())
    }

    #[test]
    fn relative_spec_urls_resolve_against_the_provider_base() {
        assert_eq!(
            resolve_provider_url("https://relay.example.com/v1", "/api/balance").unwrap(),
            "https://relay.example.com/v1/api/balance"
        );
        assert_eq!(
            resolve_provider_url("https://relay.example.com", "https://billing.example.com/q")
                .unwrap(),
            "https://billing.example.com/q"
        );
    }

    #[test]
    fn wallet_and_windows_share_the_declared_unit() {
        let provider = provider();
        let quota = evaluate_spec(
            &provider,
            &json!({ "data": { "remain": 750000, "total": 1000000 } }),
        )
        .unwrap();
        assert!(quota.known);
        assert!(quota.total_known);
        assert_eq!(quota.available, 750000.0);
        assert_eq!(quota.used, 250000.0);
        assert_eq!(quota.scope, ProviderQuotaScope::Token);
    }

    #[test]
    fn missing_balance_falls_back_to_windows_without_failing_them() {
        let mut provider = provider();
        provider.quota_spec.balance = "data.missing".to_string();
        provider.quota_spec.total.clear();
        provider.quota_spec.windows = vec![crate::models::ProviderQuotaWindowSpec {
            label: "5 小时".to_string(),
            used: "data.used".to_string(),
            limit: "data.limit".to_string(),
            resets_at: "data.reset_at".to_string(),
        }];
        let quota = evaluate_spec(
            &provider,
            &json!({ "data": { "used": 4, "limit": 10, "reset_at": 1_800_000_000 } }),
        )
        .unwrap();
        assert_eq!(quota.windows.len(), 1);
        assert_eq!(quota.windows[0].label, "5 小时");
        assert_eq!(quota.windows[0].percent(), Some(40.0));
        assert_eq!(quota.windows[0].resets_at, Some(1_800_000_000));
        assert!(!quota.total_known);
    }

    #[test]
    fn unlimited_sentinel_is_preserved() {
        let provider = provider();
        let quota =
            evaluate_spec(&provider, &json!({ "data": { "remain": -1, "total": 0 } })).unwrap();
        assert!(quota.unlimited);
        assert!(quota.known);
    }
}
