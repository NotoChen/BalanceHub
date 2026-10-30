use crate::models::{
    Provider, ProviderModelHealth, ProviderUsageModelPoint, ProviderUsageModelStat,
    ProviderUsagePoint, ProviderUsageSummary,
};
use chrono::{DateTime, Local};
use reqwest::{Method, Url};
use std::collections::BTreeMap;

use super::http::{
    build_url, build_user_request, provider_user_management_context, ProviderTransport,
    UserCredential,
};
use super::response::{
    extract_i64_field, extract_string_field, extract_usage_items, parse_success_data, send_text,
};
use super::site::{convert_quota_value, fetch_site_metadata_or, site_metadata_from_provider};

pub async fn fetch_usage_summary(
    client: &ProviderTransport,
    provider: &Provider,
    period: &str,
) -> Result<ProviderUsageSummary, String> {
    let (start_timestamp, end_timestamp, hourly) = usage_window(period);
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let site =
        fetch_site_metadata_or(client, &base_url, site_metadata_from_provider(provider)).await?;
    let url = usage_url(&base_url, start_timestamp, end_timestamp)?;
    let request = build_user_request(
        client,
        Method::GET,
        url,
        &base_url,
        &api_user,
        credential.clone(),
    );
    let (status, body) = send_text(client, request, "读取用量趋势").await?;
    let data = parse_success_data(&status, body, "用量趋势")?;

    let mut grouped: BTreeMap<String, (f64, i64, i64)> = BTreeMap::new();
    let mut model_grouped: BTreeMap<String, (f64, i64, i64)> = BTreeMap::new();
    let mut model_point_grouped: BTreeMap<(String, String), (f64, i64, i64)> = BTreeMap::new();
    for item in extract_usage_items(&data) {
        let created_at = extract_i64_field(&item, &["created_at", "createdAt"]).unwrap_or(0);
        if created_at <= 0 {
            continue;
        }
        let model_name = extract_model_name(&item);
        let quota = extract_i64_field(&item, &["quota", "Quota"]).unwrap_or(0);
        let count = extract_i64_field(&item, &["count", "Count"]).unwrap_or(0);
        let token_used = extract_i64_field(&item, &["token_used", "tokenUsed"]).unwrap_or(0);
        let date = if hourly {
            format_local_hour(created_at)
        } else {
            format_local_date(created_at)
        };
        let (used, _) = convert_quota_value(quota, &site);
        let entry = grouped.entry(date.clone()).or_insert((0.0, 0, 0));
        entry.0 += used;
        entry.1 += count;
        entry.2 += token_used;

        let model_entry = model_grouped
            .entry(model_name.clone())
            .or_insert((0.0, 0, 0));
        model_entry.0 += used;
        model_entry.1 += count;
        model_entry.2 += token_used;

        let model_point_entry = model_point_grouped
            .entry((date, model_name))
            .or_insert((0.0, 0, 0));
        model_point_entry.0 += used;
        model_point_entry.1 += count;
        model_point_entry.2 += token_used;
    }

    let (_, quota_display) = convert_quota_value(0, &site);
    let mut model_stats = model_grouped
        .into_iter()
        .map(
            |(model_name, (used, request_count, token_used))| ProviderUsageModelStat {
                model_name,
                used,
                request_count,
                token_used,
            },
        )
        .collect::<Vec<_>>();
    model_stats.sort_by(|left, right| {
        right
            .used
            .partial_cmp(&left.used)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.request_count.cmp(&left.request_count))
    });

    let model_health = fetch_model_health(client, &base_url, &api_user, credential).await;
    let request_count = grouped.values().map(|entry| entry.1).sum();

    Ok(ProviderUsageSummary {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.display_label(),
        quota_display,
        points: grouped
            .into_iter()
            .map(
                |(date, (used, request_count, token_used))| ProviderUsagePoint {
                    date,
                    used,
                    request_count,
                    token_used,
                },
            )
            .collect(),
        model_stats,
        model_points: model_point_grouped
            .into_iter()
            .map(|((date, model_name), (used, request_count, token_used))| {
                ProviderUsageModelPoint {
                    date,
                    model_name,
                    used,
                    request_count,
                    token_used,
                }
            })
            .collect(),
        model_health,
        request_count,
    })
}

/// 站点的性能采样（响应耗时、成功率、吞吐）是另一个可选能力：老站点没有这个
/// 接口，或管理员关掉了采样，就返回空列表，卡片随之不展示响应质量。
async fn fetch_model_health(
    client: &ProviderTransport,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
) -> Vec<ProviderModelHealth> {
    let Ok(url) = build_url(base_url, "/api/perf-metrics/summary?hours=24") else {
        return Vec::new();
    };
    let request = build_user_request(client, Method::GET, url, base_url, api_user, credential);
    let Ok((status, body)) = send_text(client, request, "读取响应质量").await else {
        return Vec::new();
    };
    let Ok(data) = parse_success_data(&status, body, "响应质量") else {
        return Vec::new();
    };
    parse_model_health(&data)
}

/// 只保留能被展示的行：没有模型名或者三项采样全空的行没有信息量。
fn parse_model_health(data: &serde_json::Value) -> Vec<ProviderModelHealth> {
    data.get("models")
        .and_then(|value| value.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|item| {
                    let model_name = extract_string_field(item, &["model_name", "modelName"])?;
                    let health = ProviderModelHealth {
                        model_name,
                        success_rate: item.get("success_rate").and_then(|value| value.as_f64()),
                        avg_latency_ms: extract_i64_field(
                            item,
                            &["avg_latency_ms", "avgLatencyMs"],
                        ),
                        avg_tps: item.get("avg_tps").and_then(|value| value.as_f64()),
                    };
                    (health.success_rate.is_some()
                        || health.avg_latency_ms.is_some()
                        || health.avg_tps.is_some())
                    .then_some(health)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_model_name(item: &serde_json::Value) -> String {
    extract_string_field(item, &["model_name", "modelName", "model"])
        .unwrap_or_else(|| "未知模型".to_string())
}

/// “24 小时”在两种协议下必须是同一段时间：Sub2API 的用量接口按自然日取数，
/// 这里也按自然日取（今天零点到现在），两边的小时桶才对得上。
fn usage_window(period: &str) -> (i64, i64, bool) {
    let now = crate::util::unix_secs() as i64;
    let start_of_today = now - now.rem_euclid(86_400);
    match period.trim().to_ascii_lowercase().as_str() {
        "24h" | "1d" => (start_of_today, now, true),
        days => {
            let days = if days == "7d" { 7 } else { 30 };
            (now - days * 86_400, now, false)
        }
    }
}

fn usage_url(base_url: &str, start_timestamp: i64, end_timestamp: i64) -> Result<Url, String> {
    build_url(
        base_url,
        &format!("/api/data/self?start_timestamp={start_timestamp}&end_timestamp={end_timestamp}"),
    )
}

fn format_local_hour(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .map(|datetime| {
            datetime
                .with_timezone(&Local)
                .format("%m-%d %H:00")
                .to_string()
        })
        .unwrap_or_else(|| timestamp.to_string())
}

fn format_local_date(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .map(|datetime| {
            datetime
                .with_timezone(&Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_model_health, usage_window};
    use serde_json::json;

    #[test]
    fn model_health_keeps_sampled_rows_and_skips_empty_ones() {
        let data = json!({
            "models": [
                { "model_name": "gpt-5.6-sol", "avg_latency_ms": 812, "success_rate": 99.5, "avg_tps": 61.2 },
                { "model_name": "claude-sonnet-5" },
                { "avg_latency_ms": 300, "success_rate": 90.0 }
            ]
        });
        let health = parse_model_health(&data);
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].model_name, "gpt-5.6-sol");
        assert_eq!(health[0].avg_latency_ms, Some(812));
        assert_eq!(health[0].avg_tps, Some(61.2));
    }

    #[test]
    fn sites_without_perf_sampling_report_no_health() {
        assert!(parse_model_health(&json!({ "success": true, "data": {} })).is_empty());
        assert!(parse_model_health(&json!({ "models": [] })).is_empty());
    }

    /// Sub2API 的 24 小时视图是今天零点到现在，NewAPI 必须落在同一段时间里，
    /// 否则同一个周期切换卡片会把“今天”和“过去 24 小时”混着比。
    #[test]
    fn the_day_window_starts_at_local_midnight_and_keeps_hours() {
        let (start, end, hourly) = usage_window("24h");
        let now = crate::util::unix_secs() as i64;
        assert!(hourly);
        assert_eq!(start % 86_400, 0);
        assert!(end >= start && end <= now);
        assert!(now - end < 5);
        let (week_start, week_end, hourly) = usage_window("7d");
        assert!(!hourly);
        assert_eq!(week_end - week_start, 7 * 86_400);
        let (month_start, month_end, _) = usage_window("30d");
        assert_eq!(month_end - month_start, 30 * 86_400);
    }
}
