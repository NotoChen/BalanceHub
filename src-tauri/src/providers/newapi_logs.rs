use crate::models::{
    Provider, ProviderRequestLog, ProviderRequestLogsQuery, ProviderRequestLogsResult,
};
use chrono::{DateTime, Local};
use reqwest::{Client, Method};
use serde_json::Value;

use super::newapi_http::{build_url, build_user_request, provider_user_management_context};
use super::newapi_response::{
    extract_f64_field, extract_i64_field, extract_string_field, parse_success_data, send_text,
};
use super::newapi_site::{convert_quota_value, fetch_site_metadata, site_metadata_from_provider};

pub async fn fetch_request_logs(
    client: &Client,
    provider: &Provider,
    query: ProviderRequestLogsQuery,
) -> Result<ProviderRequestLogsResult, String> {
    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    let site = fetch_site_metadata(client, &base_url, is_anyrouter)
        .await
        .unwrap_or_else(|_| site_metadata_from_provider(provider));

    let keyword = query.keyword.trim();
    let mut url = build_url(&base_url, "/api/log/self")?;
    let end_timestamp = crate::util::unix_secs();
    let start_timestamp = end_timestamp.saturating_sub(30 * 86_400);
    {
        let mut pairs = url.query_pairs_mut();
        // 前端分页是 0-based，NewAPI 接口分页是 1-based。
        pairs.append_pair("p", &(query.page + 1).to_string());
        pairs.append_pair("page_size", &query.page_size.to_string());
        pairs.append_pair("start_timestamp", &start_timestamp.to_string());
        pairs.append_pair("end_timestamp", &end_timestamp.to_string());
        if !keyword.is_empty() {
            pairs.append_pair("keyword", keyword);
        }
    }

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
    let (status, body) = send_text(request, "读取请求日志").await?;
    let data = parse_success_data(&status, body, "请求日志")?;
    let total = extract_total(&data);
    let logs = extract_log_items(&data)
        .into_iter()
        .map(|item| normalize_log_item(item, &site))
        .collect::<Vec<_>>();
    let (_, quota_display) = convert_quota_value(0, &site);

    Ok(ProviderRequestLogsResult {
        provider_id: provider.identity.id.clone(),
        provider_name: provider.identity.name.clone(),
        page: query.page,
        page_size: query.page_size,
        total,
        quota_display,
        logs,
        message: "请求日志已加载".to_string(),
    })
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

fn normalize_log_item(item: Value, site: &super::newapi_site::SiteMetadata) -> ProviderRequestLog {
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
