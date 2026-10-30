use super::{grok, http};
use crate::models::{AccountQuotaWindow, AccountSubscription, Provider};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;

// These endpoints use the published CLI wire protocol, independently of the
// user's installed executable. Keep its identity in one place.
const CLI_VERSION: &str = "1.0.46";

fn billing_request(client: &Client, provider: &Provider, credits: bool) -> RequestBuilder {
    let url = if credits {
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits"
    } else {
        "https://cli-chat-proxy.grok.com/v1/billing"
    };
    with_cli_identity(client.get(url), provider)
}

pub(super) fn with_cli_identity(request: RequestBuilder, provider: &Provider) -> RequestBuilder {
    let mut request = request
        .bearer_auth(&provider.auth.access_token)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("x-xai-token-auth", "xai-grok-cli")
        .header("x-grok-client-version", CLI_VERSION)
        .header("x-grok-client-mode", "interactive")
        .header(
            "User-Agent",
            format!(
                "grok-pager/{CLI_VERSION} grok-shell/{CLI_VERSION} ({}; {})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        );
    if let Some(identity) = &provider.auth.oauth {
        if !identity.subject.is_empty() {
            request = request.header("X-UserID", &identity.subject);
        }
        if !identity.email.is_empty() {
            request = request.header("X-Email", &identity.email);
        }
    }
    request
}

pub(super) async fn subscription(
    client: &Client,
    provider: &Provider,
) -> Result<AccountSubscription, String> {
    let (weekly, monthly) = tokio::join!(
        http::json(billing_request(client, provider, true), "读取 Grok 周额度"),
        http::json(billing_request(client, provider, false), "读取 Grok 月额度")
    );
    let weekly = weekly.and_then(|v| parse(&v));
    let monthly = monthly.and_then(|v| parse(&v));
    let mut snapshot = AccountSubscription::default();
    let mut failures = Vec::new();
    for domain in [weekly, monthly] {
        match domain {
            Ok(value) => {
                for window in value.windows {
                    snapshot.windows.retain(|old| old.id != window.id);
                    snapshot.windows.push(window);
                }
                if value.credits.is_some() {
                    snapshot.credits = value.credits;
                    snapshot.credits_unit = value.credits_unit;
                }
            }
            Err(error) => failures.push(error),
        }
    }
    if snapshot.windows.is_empty() && snapshot.credits.is_none() {
        // Read-only fallback for deployments/accounts still exposing gRPC billing.
        let bytes = http::bytes(
            client
                .post("https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig")
                .bearer_auth(&provider.auth.access_token)
                .header("Origin", "https://grok.com")
                .header("Referer", "https://grok.com/?_s=usage")
                .header("Content-Type", "application/grpc-web+proto")
                .header("x-grpc-web", "1")
                .header("x-user-agent", "connect-es/2.1.1")
                .body(vec![0u8; 5]),
            "读取 Grok 额度",
        )
        .await
        .map_err(|fallback| format!("{}；{fallback}", failures.join("；")))?;
        return grok::parse(&bytes);
    }
    snapshot.windows.sort_by_key(|w| match w.id.as_str() {
        "grok-weekly" => 0,
        "grok-monthly" => 1,
        "grok-on-demand" => 3,
        _ => 2,
    });
    snapshot.error = (!failures.is_empty()).then(|| failures.join("；"));
    Ok(snapshot)
}

fn amount(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|v| http::number(v.get("val").unwrap_or(v)))
        .filter(|v| *v >= 0.0)
}

pub(super) fn parse(value: &Value) -> Result<AccountSubscription, String> {
    let config = value
        .get("config")
        .filter(|v| v.is_object())
        .ok_or("Grok 未返回账单额度")?;
    let mut result = AccountSubscription::default();
    // Weekly resets must never be borrowed from the monthly billing end.
    let weekly_reset = config
        .pointer("/currentPeriod/end")
        .and_then(http::timestamp);
    if let Some(used_percent) = amount(config.get("creditUsagePercent")) {
        result.windows.push(AccountQuotaWindow {
            id: "grok-weekly".into(),
            label: "周额度".into(),
            used_percent: Some(used_percent),
            reset_at: weekly_reset,
            window_seconds: Some(604_800),
            ..Default::default()
        });
    }
    if let Some(products) = config.get("productUsage").and_then(Value::as_array) {
        for product in products {
            let name = http::text(product, &["product"]);
            if name.is_empty() {
                continue;
            }
            if let Some(used_percent) = amount(product.get("usagePercent")) {
                result.windows.push(AccountQuotaWindow {
                    id: format!("grok-product-{name}"),
                    label: format!("{name} · 周额度"),
                    used_percent: Some(used_percent),
                    reset_at: weekly_reset,
                    window_seconds: Some(604_800),
                    ..Default::default()
                });
            }
        }
    }
    for (id, label, used_key, limit_key, divisor) in [
        ("grok-monthly", "月额度", "used", "monthlyLimit", 100.0),
        (
            "grok-on-demand",
            "按量使用",
            "onDemandUsed",
            "onDemandCap",
            1.0,
        ),
    ] {
        let used = amount(config.get(used_key)).map(|v| v / divisor);
        let limit = amount(config.get(limit_key)).map(|v| v / divisor);
        if used.is_none() && limit.is_none() {
            continue;
        }
        let used_percent = used
            .zip(limit)
            .filter(|(_, cap)| *cap > 0.0)
            .map(|(used, cap)| used / cap * 100.0);
        result.windows.push(AccountQuotaWindow {
            id: id.into(),
            label: label.into(),
            used,
            limit,
            used_percent,
            unit: Some("USD".into()),
            reset_at: config.get("billingPeriodEnd").and_then(http::timestamp),
            ..Default::default()
        });
    }
    result.credits = amount(config.get("prepaidBalance"));
    result.credits_unit = result.credits.map(|_| "USD".into());
    if result.windows.is_empty() && result.credits.is_none() {
        return Err("Grok 尚未返回可用额度数据".into());
    }
    Ok(result)
}
