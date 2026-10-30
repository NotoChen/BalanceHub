use crate::models::ProviderQuotaWindow;
use serde_json::Value;

use super::json::{number_field, string_field, timestamp_millis};

/// 订阅窗口按日/周/月三层限额返回，卡片只为其中一行留位置，因此只保留前三个：
/// 日限额最先决定今天还能不能继续用，周/月只是补充背景。
const MAX_WINDOWS: usize = 3;

pub(super) fn subscription_windows(items: &[Value]) -> Vec<ProviderQuotaWindow> {
    let mut windows = Vec::new();
    let multiple = items.len() > 1;
    for item in items {
        let progress = item.get("progress").unwrap_or(item);
        let group = string_field(progress, &["group_name", "groupName"]).unwrap_or_default();
        let prefix = if multiple && !group.is_empty() {
            format!("{group} ")
        } else {
            String::new()
        };
        for (key, label) in [("daily", "今日"), ("weekly", "本周"), ("monthly", "本月")] {
            let Some(window) = progress.get(key).filter(|value| value.is_object()) else {
                continue;
            };
            let limit = number_field(window, &["limit_usd", "limitUsd"]);
            // Sub2API 不给上限时直接不返回该窗口；0 上限同样表示不限量而不是已用尽。
            if limit <= 0.0 {
                continue;
            }
            windows.push(ProviderQuotaWindow {
                label: format!("{prefix}{label}"),
                used: number_field(window, &["used_usd", "usedUsd"]).max(0.0),
                limit,
                resets_at: timestamp_millis(window, &["resets_at", "resetsAt"])
                    .map(|value| value / 1000),
            });
            if windows.len() >= MAX_WINDOWS {
                return windows;
            }
        }
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::subscription_windows;
    use serde_json::json;

    #[test]
    fn daily_weekly_monthly_windows_keep_their_own_limits() {
        let windows = subscription_windows(&[json!({
            "subscription": { "id": 1 },
            "progress": {
                "group_name": "Claude",
                "daily": {
                    "limit_usd": 10.0,
                    "used_usd": 2.5,
                    "resets_at": "2026-10-23T00:00:00Z"
                },
                "weekly": { "limit_usd": 40.0, "used_usd": 12.0 },
                "monthly": { "limit_usd": 120.0, "used_usd": 30.0 }
            }
        })]);

        assert_eq!(
            windows
                .iter()
                .map(|window| window.label.as_str())
                .collect::<Vec<_>>(),
            ["今日", "本周", "本月"]
        );
        assert_eq!(windows[0].limit, 10.0);
        assert_eq!(windows[0].used, 2.5);
        assert_eq!(windows[0].percent(), Some(25.0));
        assert!(windows[0].resets_at.is_some());
        assert_eq!(windows[1].percent(), Some(30.0));
    }

    #[test]
    fn multiple_subscriptions_name_the_group_so_windows_stay_distinguishable() {
        let windows = subscription_windows(&[
            json!({ "progress": { "group_name": "Claude", "daily": { "limit_usd": 5.0 } } }),
            json!({ "progress": { "group_name": "Codex", "daily": { "limit_usd": 8.0 } } }),
        ]);

        assert_eq!(windows[0].label, "Claude 今日");
        assert_eq!(windows[1].label, "Codex 今日");
    }

    #[test]
    fn unlimited_and_missing_windows_do_not_become_empty_gauges() {
        let windows = subscription_windows(&[json!({
            "progress": { "daily": { "limit_usd": 0, "used_usd": 3.0 } }
        })]);

        assert!(windows.is_empty());
    }

    #[test]
    fn window_list_is_capped_so_the_card_row_stays_one_line() {
        let windows = subscription_windows(&[
            json!({ "progress": { "group_name": "A", "daily": { "limit_usd": 1.0 }, "weekly": { "limit_usd": 2.0 } } }),
            json!({ "progress": { "group_name": "B", "daily": { "limit_usd": 3.0 }, "weekly": { "limit_usd": 4.0 } } }),
        ]);

        assert_eq!(windows.len(), 3);
    }
}
