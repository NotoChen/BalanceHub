use serde_json::Value;

use crate::{
    adapters::new_api::{
        response::{extract_i64_field, extract_string_field},
        site::{convert_quota_value, SiteMetadata},
    },
    models::ProviderQuotaWindow,
};

/// 卡片只给订阅窗口留一行，多出来的订阅在详情提示里也只是重复的“订阅 N”，
/// 因此一次性收口到三个窗口，避免存储和渲染被无限订阅历史撑大。
const MAX_WINDOWS: usize = 3;

/// NewAPI 的订阅额度以 quota 单位返回，和钱包余额共用 `quota_per_unit`；
/// 只有转换为同一个展示单位，卡片上的钱包数字和订阅窗口才能直接比较。
pub(super) fn subscription_windows(data: &Value, site: &SiteMetadata) -> Vec<ProviderQuotaWindow> {
    let items = data
        .get("subscriptions")
        .or_else(|| data.get("all_subscriptions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let entries = items
        .iter()
        .filter_map(|item| active_subscription(item))
        .collect::<Vec<_>>();
    entries
        .iter()
        .take(MAX_WINDOWS)
        .enumerate()
        .map(|(index, subscription)| {
            let total =
                extract_i64_field(subscription, &["amount_total", "amountTotal"]).unwrap_or(0);
            let used = extract_i64_field(subscription, &["amount_used", "amountUsed"]).unwrap_or(0);
            // 0 是 NewAPI 的“不限额度”，与钱包的 unlimited 语义一致。
            let limit = if total > 0 {
                convert_quota_value(total, site).0
            } else {
                0.0
            };
            ProviderQuotaWindow {
                label: if entries.len() > 1 {
                    format!("订阅{}", index + 1)
                } else {
                    "订阅".to_string()
                },
                used: convert_quota_value(used.max(0), site).0,
                limit,
                resets_at: subscription_reset_at(subscription),
            }
        })
        .collect()
}

fn active_subscription(item: &Value) -> Option<&Value> {
    let subscription = item.get("subscription").unwrap_or(item);
    let status = extract_string_field(subscription, &["status"]).unwrap_or_default();
    if !status.is_empty() && !status.eq_ignore_ascii_case("active") {
        return None;
    }
    Some(subscription)
}

/// 周期订阅返回 `next_reset_time`，一次性订阅只有 `end_time`；两者都是秒级时间戳。
fn subscription_reset_at(subscription: &Value) -> Option<i64> {
    ["next_reset_time", "nextResetTime", "end_time", "endTime"]
        .iter()
        .filter_map(|field| extract_i64_field(subscription, &[*field]))
        .find(|value| *value > 0)
        .map(|value| {
            if value >= 1_000_000_000_000 {
                value / 1000
            } else {
                value
            }
        })
}

#[cfg(test)]
mod tests {
    use super::subscription_windows;
    use crate::adapters::new_api::site::SiteMetadata;
    use serde_json::json;

    fn site() -> SiteMetadata {
        SiteMetadata {
            quota_per_unit: 500_000.0,
            ..SiteMetadata::default()
        }
    }

    #[test]
    fn active_subscriptions_become_quota_windows_in_display_units() {
        let windows = subscription_windows(
            &json!({
                "billing_preference": "subscription",
                "subscriptions": [
                    {
                        "subscription": {
                            "id": 7,
                            "status": "active",
                            "amount_total": 5_000_000,
                            "amount_used": 1_000_000,
                            "next_reset_time": 1_800_000_000
                        }
                    }
                ]
            }),
            &site(),
        );

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "订阅");
        assert_eq!(windows[0].limit, 10.0);
        assert_eq!(windows[0].used, 2.0);
        assert_eq!(windows[0].resets_at, Some(1_800_000_000));
        assert_eq!(windows[0].percent(), Some(20.0));
    }

    #[test]
    fn zero_total_subscription_is_unlimited_instead_of_empty() {
        let windows = subscription_windows(
            &json!({
                "subscriptions": [
                    { "subscription": { "status": "active", "amount_total": 0, "amount_used": 0 } }
                ]
            }),
            &site(),
        );

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].limit, 0.0);
        assert_eq!(windows[0].percent(), None);
    }

    #[test]
    fn expired_subscriptions_are_not_shown_as_current_quota() {
        let windows = subscription_windows(
            &json!({
                "subscriptions": [
                    { "subscription": { "status": "expired", "amount_total": 1, "amount_used": 1 } }
                ]
            }),
            &site(),
        );

        assert!(windows.is_empty());
    }

    #[test]
    fn millisecond_reset_times_are_normalized_to_seconds() {
        let windows = subscription_windows(
            &json!({
                "subscriptions": [
                    {
                        "subscription": {
                            "status": "active",
                            "amount_total": 1,
                            "end_time": 1_800_000_000_000_i64
                        }
                    }
                ]
            }),
            &site(),
        );

        assert_eq!(windows[0].resets_at, Some(1_800_000_000));
    }
}
