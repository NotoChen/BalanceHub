use crate::models::{
    Provider, ProviderRequestLog, ProviderRequestLogStats, ProviderRequestLogsQuery,
    ProviderRequestLogsResult,
};
use chrono::{DateTime, Local};
use reqwest::{Client, Method};
use serde_json::Value;

use super::newapi_http::{
    build_url, build_user_request, provider_user_management_context, UserCredential,
};
use super::newapi_response::{
    extract_f64_field, extract_i64_field, extract_string_field, parse_success_data, send_text,
};
use super::newapi_site::{
    convert_quota_value, fetch_site_metadata, site_metadata_from_provider, SiteMetadata,
};

struct RequestLogStatsContext<'a> {
    client: &'a Client,
    base_url: &'a str,
    api_user: &'a str,
    credential: UserCredential,
    is_anyrouter: bool,
    site: &'a SiteMetadata,
}

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

    let end_timestamp = crate::util::unix_secs();
    let start_timestamp = end_timestamp.saturating_sub(30 * 86_400);
    let stats = fetch_request_log_stats(
        RequestLogStatsContext {
            client,
            base_url: &base_url,
            api_user: &api_user,
            credential: credential.clone(),
            is_anyrouter,
            site: &site,
        },
        &query,
        start_timestamp,
        end_timestamp,
    )
    .await
    .unwrap_or_default();

    let url = request_logs_url(&base_url, &query, start_timestamp, end_timestamp, true)?;

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
        stats,
        logs,
        message: "请求日志已加载".to_string(),
    })
}

async fn fetch_request_log_stats(
    context: RequestLogStatsContext<'_>,
    query: &ProviderRequestLogsQuery,
    start_timestamp: u64,
    end_timestamp: u64,
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
        context.is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "读取请求日志统计").await?;
    let data = parse_success_data(&status, body, "请求日志统计")?;
    let raw_quota = extract_i64_field(&data, &["quota", "Quota"])
        .or_else(|| extract_f64_field(&data, &["quota", "Quota"]).map(|value| value as i64))
        .unwrap_or(0);
    Ok(ProviderRequestLogStats {
        quota: convert_quota_value(raw_quota, context.site).0,
        rpm: extract_f64_field(&data, &["rpm", "RPM"]).unwrap_or_default(),
        tpm: extract_f64_field(&data, &["tpm", "TPM"]).unwrap_or_default(),
    })
}

fn request_logs_url(
    base_url: &str,
    query: &ProviderRequestLogsQuery,
    start_timestamp: u64,
    end_timestamp: u64,
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
        let keyword = query.keyword.trim();
        if !keyword.is_empty() {
            // NewAPI 官方日志接口没有通用 keyword 参数。这里把单框搜索映射到最常用的
            // model_name，避免继续发送无效参数；精确筛选后续再单独拆成多字段控件。
            pairs.append_pair("model_name", keyword);
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
