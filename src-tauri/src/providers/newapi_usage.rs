use crate::models::{Provider, ProviderUsagePoint, ProviderUsageSummary};
use chrono::{DateTime, Local};
use reqwest::{Client, Method};
use std::collections::BTreeMap;

use super::newapi_http::{build_url, build_user_request, provider_user_management_context};
use super::newapi_response::{
    extract_i64_field, extract_usage_items, parse_success_data, send_text,
};
use super::newapi_site::{convert_quota_value, fetch_site_metadata, site_metadata_from_provider};

pub async fn fetch_usage_summary(
    client: &Client,
    provider: &Provider,
    period: &str,
) -> Result<ProviderUsageSummary, String> {
    let (seconds, hourly) = usage_period(period);
    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    let site = fetch_site_metadata(client, &base_url, is_anyrouter)
        .await
        .unwrap_or_else(|_| site_metadata_from_provider(provider));
    let end_timestamp = crate::util::unix_secs() as i64;
    let start_timestamp = end_timestamp - seconds;
    let url = build_url(
        &base_url,
        &format!("/api/data/self?start_timestamp={start_timestamp}&end_timestamp={end_timestamp}"),
    )?;
    let request = build_user_request(
        client,
        Method::GET,
        url,
        &base_url,
        &api_user,
        credential,
        is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "读取用量趋势").await?;
    let data = parse_success_data(&status, body, "用量趋势")?;

    let mut grouped: BTreeMap<String, (f64, i64, i64)> = BTreeMap::new();
    for item in extract_usage_items(&data) {
        let created_at = extract_i64_field(&item, &["created_at", "createdAt"]).unwrap_or(0);
        if created_at <= 0 {
            continue;
        }
        let quota = extract_i64_field(&item, &["quota", "Quota"]).unwrap_or(0);
        let count = extract_i64_field(&item, &["count", "Count"]).unwrap_or(0);
        let token_used = extract_i64_field(&item, &["token_used", "tokenUsed"]).unwrap_or(0);
        let date = if hourly {
            format_local_hour(created_at)
        } else {
            format_local_date(created_at)
        };
        let (used, _) = convert_quota_value(quota, &site);
        let entry = grouped.entry(date).or_insert((0.0, 0, 0));
        entry.0 += used;
        entry.1 += count;
        entry.2 += token_used;
    }

    let (_, quota_display) = convert_quota_value(0, &site);
    Ok(ProviderUsageSummary {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
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
    })
}

fn usage_period(period: &str) -> (i64, bool) {
    match period.trim().to_ascii_lowercase().as_str() {
        "24h" | "1d" => (86_400, true),
        "7d" => (7 * 86_400, false),
        "30d" => (30 * 86_400, false),
        _ => (30 * 86_400, false),
    }
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
