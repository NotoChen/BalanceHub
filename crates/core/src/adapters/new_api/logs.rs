use crate::models::{
    Provider, ProviderRequestLog, ProviderRequestLogStats, ProviderRequestLogsQuery,
    ProviderRequestLogsResult,
};
use chrono::{DateTime, Local};
use reqwest::Method;
use serde_json::Value;

use super::http::{
    build_url, build_user_request, provider_user_management_context, ProviderTransport,
    UserCredential,
};
use super::response::{
    extract_f64_field, extract_i64_field, extract_string_field, parse_success_data, send_text,
};
use super::site::{
    convert_quota_value, fetch_site_metadata_or, site_metadata_from_provider, SiteMetadata,
};

struct RequestLogStatsContext<'a> {
    client: &'a ProviderTransport,
    base_url: &'a str,
    api_user: &'a str,
    credential: UserCredential,
    site: &'a SiteMetadata,
}

pub async fn fetch_request_logs(
    client: &ProviderTransport,
    provider: &Provider,
    query: ProviderRequestLogsQuery,
) -> Result<ProviderRequestLogsResult, String> {
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let site =
        fetch_site_metadata_or(client, &base_url, site_metadata_from_provider(provider)).await?;

    let end_timestamp = query
        .end_timestamp
        .unwrap_or_else(|| crate::util::unix_secs() as i64)
        .max(0);
    let start_timestamp = query
        .start_timestamp
        .unwrap_or_else(|| end_timestamp.saturating_sub(30 * 86_400))
        .max(0)
        .min(end_timestamp);
    let stats = fetch_request_log_stats(
        RequestLogStatsContext {
            client,
            base_url: &base_url,
            api_user: &api_user,
            credential: credential.clone(),
            site: &site,
        },
        &query,
        start_timestamp,
        end_timestamp,
    )
    .await
    .unwrap_or_default();

    let url = request_logs_url(&base_url, &query, start_timestamp, end_timestamp, true)?;

    let request = build_user_request(client, Method::GET, url, &base_url, &api_user, credential);
    let (status, body) = send_text(client, request, "读取请求日志").await?;
    let data = parse_success_data(&status, body, "请求日志")?;
    let total = extract_total(&data);
    let logs = extract_log_items(&data)
        .into_iter()
        .map(|item| normalize_log_item(item, &site))
        .collect::<Vec<_>>();
    let (_, quota_display) = convert_quota_value(0, &site);

    Ok(ProviderRequestLogsResult {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.display_label(),
        page: query.page,
        page_size: query.page_size,
        total,
        quota_display,
        stats,
        logs,
        message: "请求日志已加载".to_string(),
    })
}

async fn fetch_request_log_stats(
    context: RequestLogStatsContext<'_>,
    query: &ProviderRequestLogsQuery,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<ProviderRequestLogStats, String> {
    let url = request_logs_url(
        context.base_url,
        query,
        start_timestamp,
        end_timestamp,
        false,
    )?;
    let request = build_user_request(
        context.client,
        Method::GET,
        url,
        context.base_url,
        context.api_user,
        context.credential,
    );
    let (status, body) = send_text(context.client, request, "读取请求日志统计").await?;
    let data = parse_success_data(&status, body, "请求日志统计")?;
    let raw_quota = extract_i64_field(&data, &["quota", "Quota"])
        .or_else(|| extract_f64_field(&data, &["quota", "Quota"]).map(|value| value as i64))
        .unwrap_or(0);
    Ok(ProviderRequestLogStats {
        quota: Some(convert_quota_value(raw_quota, context.site).0),
        rpm: extract_f64_field(&data, &["rpm", "RPM"]),
        tpm: extract_f64_field(&data, &["tpm", "TPM"]),
        avg_duration_ms: None,
    })
}

fn request_logs_url(
    base_url: &str,
    query: &ProviderRequestLogsQuery,
    start_timestamp: i64,
    end_timestamp: i64,
    include_pagination: bool,
) -> Result<reqwest::Url, String> {
    let mut url = build_url(
        base_url,
        if include_pagination {
            "/api/log/self"
        } else {
            "/api/log/self/stat"
        },
    )?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("start_timestamp", &start_timestamp.to_string());
        pairs.append_pair("end_timestamp", &end_timestamp.to_string());
        // NewAPI 没有通用 keyword 参数；每个维度都有自己的字段名，空值不能发送，
        // 否则部分站点会把空字符串当成一个真实的筛选条件。
        for (field, value) in [
            ("model_name", query.model.trim()),
            ("token_name", query.token_name.trim()),
            ("group", query.group.trim()),
            ("request_id", query.request_id.trim()),
        ] {
            if !value.is_empty() {
                pairs.append_pair(field, value);
            }
        }
        if let Some(log_type) = query.log_type.filter(|log_type| *log_type > 0) {
            pairs.append_pair("type", &log_type.to_string());
        }
        if include_pagination {
            // 前端分页是 0-based，NewAPI 接口分页是 1-based。
            pairs.append_pair("p", &(query.page + 1).to_string());
            pairs.append_pair("page_size", &query.page_size.to_string());
        }
    }
    Ok(url)
}

fn extract_log_items(data: &Value) -> Vec<Value> {
    for key in ["items", "logs", "records", "rows", "list"] {
        if let Some(items) = data.get(key).and_then(Value::as_array) {
            return items.clone();
        }
    }

    if let Some(items) = data.as_array() {
        return items.clone();
    }

    Vec::new()
}

fn extract_total(data: &Value) -> Option<i64> {
    extract_i64_field(
        data,
        &[
            "total",
            "total_count",
            "totalCount",
            "count",
            "record_count",
        ],
    )
}

fn normalize_log_item(item: Value, site: &super::site::SiteMetadata) -> ProviderRequestLog {
    let raw_quota = extract_i64_field(&item, &["quota", "Quota"])
        .or_else(|| extract_f64_field(&item, &["quota", "Quota"]).map(|value| value as i64))
        .unwrap_or(0);
    let (quota, _) = convert_quota_value(raw_quota, site);
    let prompt_tokens = extract_i64_field(
        &item,
        &[
            "prompt_tokens",
            "promptTokens",
            "input_tokens",
            "inputTokens",
            "prompt_token",
        ],
    )
    .unwrap_or(0);
    let completion_tokens = extract_i64_field(
        &item,
        &[
            "completion_tokens",
            "completionTokens",
            "output_tokens",
            "outputTokens",
            "completion_token",
        ],
    )
    .unwrap_or(0);
    let token_used = extract_i64_field(&item, &["token_used", "tokenUsed", "tokens", "token"])
        .unwrap_or(prompt_tokens + completion_tokens);

    ProviderRequestLog {
        id: extract_string_field(&item, &["id", "log_id", "logId"]).unwrap_or_default(),
        created_at: format_log_time(
            extract_i64_field(
                &item,
                &["created_at", "createdAt", "created_time", "createdTime"],
            ),
            extract_string_field(
                &item,
                &["created_at", "createdAt", "created_time", "createdTime"],
            ),
        ),
        token_name: extract_string_field(&item, &["token_name", "tokenName", "token"])
            .unwrap_or_default(),
        model_name: extract_string_field(
            &item,
            &["model_name", "modelName", "model", "model_id", "modelId"],
        )
        .unwrap_or_default(),
        request_id: extract_string_field(&item, &["request_id", "requestId"]).unwrap_or_default(),
        status: extract_string_field(&item, &["status", "type", "result", "code"])
            .unwrap_or_default(),
        prompt_tokens,
        completion_tokens,
        token_used,
        quota,
        channel: extract_string_field(&item, &["channel_name", "channelName", "channel"])
            .unwrap_or_default(),
        duration_ms: normalize_duration_ms(&item),
        content: extract_string_field(&item, &["content", "message", "prompt", "detail"])
            .unwrap_or_default(),
        raw: item,
    }
}

fn normalize_duration_ms(item: &Value) -> Option<i64> {
    extract_i64_field(item, &["duration_ms", "durationMs", "elapsed_ms"]).or_else(|| {
        extract_f64_field(item, &["use_time", "useTime"])
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|seconds| (seconds * 1000.0).round() as i64)
    })
}

fn format_log_time(timestamp: Option<i64>, fallback: Option<String>) -> String {
    let Some(timestamp) = timestamp else {
        return fallback.unwrap_or_default();
    };
    let seconds = if timestamp > 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };

    DateTime::from_timestamp(seconds, 0)
        .map(|datetime| {
            datetime
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| fallback.unwrap_or_else(|| timestamp.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> ProviderRequestLogsQuery {
        ProviderRequestLogsQuery {
            start_timestamp: Some(1_700_000_000),
            end_timestamp: Some(1_700_086_400),
            ..ProviderRequestLogsQuery::default()
        }
    }

    #[test]
    fn every_supported_filter_is_sent_with_its_own_field_name() {
        let query = ProviderRequestLogsQuery {
            model: "claude-sonnet-5".into(),
            token_name: "fixture-key".into(),
            group: "vip".into(),
            request_id: "req-42".into(),
            log_type: Some(2),
            ..query()
        };
        let url = request_logs_url(
            "https://relay.example.com",
            &query,
            1_700_000_000,
            1_700_086_400,
            true,
        )
        .unwrap();
        let params = url.query_pairs().collect::<Vec<_>>();
        let value = |name: &str| {
            params
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.to_string())
        };
        assert_eq!(value("model_name").as_deref(), Some("claude-sonnet-5"));
        assert_eq!(value("token_name").as_deref(), Some("fixture-key"));
        assert_eq!(value("group").as_deref(), Some("vip"));
        assert_eq!(value("request_id").as_deref(), Some("req-42"));
        assert_eq!(value("type").as_deref(), Some("2"));
        assert_eq!(value("p").as_deref(), Some("1"));
        assert_eq!(value("page_size").as_deref(), Some("20"));
    }

    #[test]
    fn empty_filters_are_not_sent_as_blank_conditions() {
        let url = request_logs_url(
            "https://relay.example.com",
            &query(),
            1_700_000_000,
            1_700_086_400,
            false,
        )
        .unwrap();
        assert!(url.path().ends_with("/api/log/self/stat"));
        let params = url
            .query_pairs()
            .map(|(key, _)| key.to_string())
            .collect::<Vec<_>>();
        assert!(!params.iter().any(|key| matches!(
            key.as_str(),
            "model_name" | "token_name" | "group" | "request_id" | "type" | "p"
        )));
    }

    #[test]
    fn non_positive_log_types_are_treated_as_all() {
        let query = ProviderRequestLogsQuery {
            log_type: Some(0),
            ..query()
        };
        let url = request_logs_url(
            "https://relay.example.com",
            &query,
            1_700_000_000,
            1_700_086_400,
            true,
        )
        .unwrap();
        assert!(!url.query_pairs().any(|(key, _)| key == "type"));
    }
}
