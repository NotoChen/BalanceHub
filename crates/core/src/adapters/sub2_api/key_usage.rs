use crate::{
    adapters::transport::ProviderTransport,
    models::{Provider, ProviderQuotaWindow},
};
use reqwest::{header::ACCEPT, header::USER_AGENT};
use std::time::Duration;

use super::{
    json::{number_field, string_field, timestamp_millis},
    response::{normalize_base_url, parse_response},
};

/// Sub2API 给 API Key 单独开放了 `/v1/usage`：Key 自己的总额度、5 小时 / 1 天 /
/// 7 天消费限额，以及订阅分组或钱包口径的余额，都能只用 Key 读到，不需要账号
/// 登录。这是 Key 卡片在“模型列表”之外真正的额度来源。
pub(super) struct KeyQuota {
    pub available: f64,
    pub used: f64,
    pub total_known: bool,
    pub windows: Vec<ProviderQuotaWindow>,
}

/// 读取失败只返回 None：Key 仍可用于模型调用，卡片退回“额度未公开”，
/// 不把一次补充请求的失败当成整张卡片异常。
pub(super) async fn fetch_key_quota(
    client: &ProviderTransport,
    provider: &Provider,
) -> Option<KeyQuota> {
    let key = provider.auth.api_key.trim();
    if key.is_empty() {
        return None;
    }
    let url = format!(
        "{}/v1/usage",
        normalize_base_url(&provider.identity.base_url)
    );
    let request = client
        .get(url)
        .bearer_auth(key)
        .header(USER_AGENT, crate::adapters::transport::USER_AGENT_VALUE)
        .header(ACCEPT, "application/json")
        .timeout(Duration::from_secs(8));
    let response = client
        .send(request, "读取 Sub2API API Key 额度")
        .await
        .ok()?;
    parse_response(response.status, &response.body, "读取 Sub2API API Key 额度")
        .ok()
        .and_then(|value| key_quota_from_usage(&value))
}

const MAX_WINDOWS: usize = 3;

pub(super) fn key_quota_from_usage(value: &serde_json::Value) -> Option<KeyQuota> {
    if value.get("isValid").and_then(serde_json::Value::as_bool) == Some(false) {
        return None;
    }
    match string_field(value, &["mode"]).as_deref() {
        // Key 配了总额度或周期限额：额度以 Key 自己为准。
        Some("quota_limited") => {
            let quota = value.get("quota").filter(|quota| quota.is_object());
            let limit = quota
                .map(|quota| number_field(quota, &["limit"]))
                .unwrap_or(0.0);
            let used = quota
                .map(|quota| number_field(quota, &["used"]))
                .unwrap_or(0.0);
            let remaining = quota
                .map(|quota| number_field(quota, &["remaining"]))
                .unwrap_or(0.0);
            let windows = rate_limit_windows(value);
            if limit <= 0.0 && windows.is_empty() {
                return None;
            }
            Some(KeyQuota {
                available: remaining.max(0.0),
                used: used.max(0.0),
                total_known: limit > 0.0,
                windows,
            })
        }
        // 没有 Key 级限额时，订阅分组给出日 / 周 / 月额度，钱包分组给出余额。
        Some(_) | None => {
            let subscription = value
                .get("subscription")
                .filter(|subscription| subscription.is_object());
            if let Some(subscription) = subscription {
                let windows = subscription_windows(subscription);
                if !windows.is_empty() {
                    // `remaining` 在响应根上；-1 表示该分组没有任何周期限额。
                    return Some(KeyQuota {
                        available: number_field(value, &["remaining"]).max(0.0),
                        used: 0.0,
                        total_known: false,
                        windows,
                    });
                }
            }
            value.get("balance").map(|balance| KeyQuota {
                available: balance.as_f64().unwrap_or(0.0),
                used: 0.0,
                total_known: false,
                windows: Vec::new(),
            })
        }
    }
}

fn rate_limit_windows(value: &serde_json::Value) -> Vec<ProviderQuotaWindow> {
    let Some(items) = value
        .get("rate_limits")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let limit = number_field(item, &["limit"]);
            // 0 上限在 Sub2API 表示不启用该窗口，不是“已用尽”。
            if limit <= 0.0 {
                return None;
            }
            let window = string_field(item, &["window"])?;
            let label = match window.as_str() {
                "5h" => "5 小时",
                "1d" => "今日",
                "7d" => "本周",
                _ => return None,
            };
            Some(ProviderQuotaWindow {
                label: label.to_string(),
                used: number_field(item, &["used"]).max(0.0),
                limit,
                resets_at: timestamp_millis(item, &["reset_at", "resetAt"]).map(|at| at / 1000),
            })
        })
        .take(MAX_WINDOWS)
        .collect()
}

fn subscription_windows(subscription: &serde_json::Value) -> Vec<ProviderQuotaWindow> {
    [("今日", "daily"), ("本周", "weekly"), ("本月", "monthly")]
        .into_iter()
        .filter_map(|(label, period)| {
            let limit = number_field(subscription, &[&format!("{period}_limit_usd")]);
            if limit <= 0.0 {
                return None;
            }
            Some(ProviderQuotaWindow {
                label: label.to_string(),
                used: number_field(subscription, &[&format!("{period}_usage_usd")]).max(0.0),
                limit,
                resets_at: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::key_quota_from_usage;
    use serde_json::json;

    #[test]
    fn key_level_quota_and_rate_limits_become_card_windows() {
        let quota = key_quota_from_usage(&json!({
            "mode": "quota_limited",
            "isValid": true,
            "quota": { "limit": 20, "used": 6.5, "remaining": 13.5, "unit": "USD" },
            "rate_limits": [
                { "window": "5h", "limit": 5, "used": 1.5, "reset_at": "2026-10-22T20:00:00Z" },
                { "window": "1d", "limit": 10, "used": 4, "reset_at": "2026-10-23T00:00:00Z" },
                { "window": "7d", "limit": 40, "used": 30 }
            ]
        }))
        .expect("quota should map");

        assert_eq!(quota.available, 13.5);
        assert_eq!(quota.used, 6.5);
        assert!(quota.total_known);
        assert_eq!(
            quota
                .windows
                .iter()
                .map(|window| window.label.as_str())
                .collect::<Vec<_>>(),
            ["5 小时", "今日", "本周"]
        );
        assert_eq!(quota.windows[0].percent(), Some(30.0));
        assert_eq!(quota.windows[2].percent(), Some(75.0));
        assert!(quota.windows[0].resets_at.is_some());
        assert!(quota.windows[2].resets_at.is_none());
    }

    #[test]
    fn unrestricted_subscription_limits_and_wallet_balance_are_read_from_the_key() {
        let subscription = key_quota_from_usage(&json!({
            "mode": "unrestricted",
            "planName": "Claude",
            "remaining": 7.5,
            "subscription": {
                "daily_limit_usd": 10, "daily_usage_usd": 2,
                "weekly_limit_usd": 40, "weekly_usage_usd": 12
            }
        }))
        .expect("subscription should map");
        assert_eq!(subscription.available, 7.5);
        assert_eq!(
            subscription
                .windows
                .iter()
                .map(|window| window.label.as_str())
                .collect::<Vec<_>>(),
            ["今日", "本周"]
        );
        assert_eq!(subscription.windows[1].percent(), Some(30.0));
        assert!(!subscription.total_known);

        let wallet = key_quota_from_usage(&json!({
            "mode": "unrestricted",
            "planName": "钱包余额",
            "balance": 42.5
        }))
        .expect("wallet should map");
        assert_eq!(wallet.available, 42.5);
        assert!(wallet.windows.is_empty());
    }

    #[test]
    fn zero_limits_and_invalid_keys_do_not_become_gauge_rows() {
        let disabled = key_quota_from_usage(&json!({
            "mode": "quota_limited",
            "quota": { "limit": 0, "used": 0, "remaining": 0 },
            "rate_limits": [{ "window": "5h", "limit": 0, "used": 3 }]
        }));
        assert!(disabled.is_none());
        assert!(
            key_quota_from_usage(&json!({ "isValid": false, "mode": "quota_limited" })).is_none()
        );
    }
}
