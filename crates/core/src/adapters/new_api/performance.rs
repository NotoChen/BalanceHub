//! NewAPI 模型运行表现。
//!
//! `/api/perf-metrics/summary?hours=24` 返回最近窗口内每个模型的请求成功率、
//! 平均延迟和输出吞吐。接口挂在 `HeaderNavModulePublicOrUserAuth("pricing")`
//! 下：站点把模型广场设为公开时匿名可读，否则需要登录。这里先按卡片凭据请求，
//! 被拒后像价目接口一样退回匿名，避免把认证策略差异当成站点不支持。

use crate::{
    adapters::transport::ProviderTransport,
    limits,
    models::{
        Provider, ProviderModelPerformance, ProviderOptionalCapability,
        ProviderOptionalCapabilityState, ProviderPerformancePoint, ProviderPerformanceSnapshot,
    },
};
use reqwest::{
    header::{ACCEPT, ORIGIN, REFERER, USER_AGENT},
    Method,
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

use super::http::{
    build_url, build_user_request, normalize_base_url, provider_user_management_context,
    USER_AGENT_VALUE,
};
use super::response::{send_text, trim_message};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const WINDOW_HOURS: u32 = 24;

pub struct PerformanceProbe {
    pub snapshot: ProviderPerformanceSnapshot,
    pub state: ProviderOptionalCapabilityState,
}

#[derive(Debug)]
enum PerformanceFailure {
    /// 站点明确没有这个接口或没有开启模型广场。这是能力结论，不是请求失败。
    Unsupported(String),
    /// 网络、认证或响应格式问题；保留旧快照，不把能力标记为不支持。
    Failed(String),
}

/// 读取模型表现；能力探测和普通刷新共用。网络或认证失败保留上一次成功读数，
/// 站点明确不支持时清空旧数据。
pub async fn probe_performance(
    client: &ProviderTransport,
    provider: &Provider,
) -> PerformanceProbe {
    let mut snapshot = provider.capabilities.performance.clone();
    match fetch_performance(client, provider).await {
        Ok(loaded) => {
            let count = loaded.models.len();
            snapshot.mark_loaded(loaded.models, loaded.window_start, loaded.window_end);
            PerformanceProbe {
                snapshot,
                state: ProviderOptionalCapabilityState::supported(
                    ProviderOptionalCapability::Performance,
                    format!("已读取 {count} 个模型的近 {WINDOW_HOURS} 小时表现"),
                ),
            }
        }
        Err(PerformanceFailure::Unsupported(reason)) => {
            snapshot.mark_unsupported(reason.clone());
            PerformanceProbe {
                snapshot,
                state: ProviderOptionalCapabilityState::unsupported(
                    ProviderOptionalCapability::Performance,
                    reason,
                ),
            }
        }
        Err(PerformanceFailure::Failed(error)) => {
            snapshot.mark_error(error.clone());
            PerformanceProbe {
                snapshot,
                state: ProviderOptionalCapabilityState::unknown(
                    ProviderOptionalCapability::Performance,
                    error,
                ),
            }
        }
    }
}

#[derive(Debug)]
struct LoadedPerformance {
    models: BTreeMap<String, ProviderModelPerformance>,
    window_start: Option<i64>,
    window_end: Option<i64>,
}

async fn fetch_performance(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<LoadedPerformance, PerformanceFailure> {
    let base_url = normalize_base_url(&provider.identity.base_url);
    if base_url.is_empty() {
        return Err(PerformanceFailure::Failed("缺少中转站地址".to_string()));
    }
    let url = build_url(
        &base_url,
        &format!("/api/perf-metrics/summary?hours={WINDOW_HOURS}"),
    )
    .map_err(PerformanceFailure::Failed)?;
    let authenticated = provider_user_management_context(provider).is_ok();
    let (mut status, mut body) =
        send_performance(client, provider, url.clone(), &base_url, authenticated).await?;
    if authenticated && matches!(status.as_u16(), 401 | 403) {
        // 模型广场在多数站点是公开的；账号凭据被拒时用匿名请求再试一次。
        (status, body) = send_performance(client, provider, url, &base_url, false).await?;
    }

    if matches!(status.as_u16(), 404 | 501) {
        return Err(PerformanceFailure::Unsupported(format!(
            "站点未提供性能接口（HTTP {}）",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        return Err(PerformanceFailure::Failed(format!(
            "HTTP {}: {}",
            status.as_u16(),
            trim_message(&body)
        )));
    }
    parse_performance_body(&body)
}

async fn send_performance(
    client: &ProviderTransport,
    provider: &Provider,
    url: reqwest::Url,
    base_url: &str,
    authenticated: bool,
) -> Result<(reqwest::StatusCode, String), PerformanceFailure> {
    let request = if authenticated {
        let (_, api_user, credential) =
            provider_user_management_context(provider).map_err(PerformanceFailure::Failed)?;
        build_user_request(client, Method::GET, url, base_url, &api_user, credential)
    } else {
        client
            .get(url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ORIGIN, base_url)
            .header(REFERER, format!("{base_url}/"))
    };
    send_text(client, request.timeout(REQUEST_TIMEOUT), "读取模型表现")
        .await
        .map_err(PerformanceFailure::Failed)
}

fn parse_performance_body(body: &str) -> Result<LoadedPerformance, PerformanceFailure> {
    let decoded = serde_json::from_str::<Value>(body)
        .map_err(|error| PerformanceFailure::Failed(format!("解析模型表现失败: {error}")))?;
    if decoded.get("success").and_then(Value::as_bool) != Some(true) {
        let message = decoded
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("接口返回失败")
            .trim()
            .to_string();
        return if looks_unsupported(&message) {
            Err(PerformanceFailure::Unsupported(message))
        } else {
            Err(PerformanceFailure::Failed(message))
        };
    }
    let Some(data) = decoded.get("data").filter(|data| data.is_object()) else {
        return Err(PerformanceFailure::Unsupported(
            "站点性能接口没有返回模型表现".to_string(),
        ));
    };

    let series_map = data
        .get("recent_success_series")
        .or_else(|| decoded.get("recent_success_series"));
    let mut models = BTreeMap::new();
    for item in data
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = item
            .get("model_name")
            .or_else(|| item.get("modelName"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let recent_success = item
            .get("recent_success_series")
            .and_then(Value::as_array)
            .or_else(|| {
                series_map
                    .and_then(|series| series.get(name))
                    .and_then(Value::as_array)
            })
            .map(|series| success_series(series))
            .unwrap_or_default();
        let performance = ProviderModelPerformance {
            success_rate: percentage_field(item, &["success_rate", "successRate"]),
            avg_latency_ms: positive_f64_field(item, &["avg_latency_ms", "avgLatencyMs"]),
            avg_tps: positive_f64_field(item, &["avg_tps", "avgTps"]),
            recent_success,
        };
        if performance.has_metrics() {
            models.insert(name.to_string(), performance);
        }
    }
    models = models
        .into_iter()
        .take(limits::MAX_MODELS_PER_PROVIDER)
        .collect();

    Ok(LoadedPerformance {
        models,
        window_start: i64_field(data, &["window_start", "windowStart"]),
        window_end: i64_field(data, &["window_end", "windowEnd"]),
    })
}

fn success_series(items: &[Value]) -> Vec<ProviderPerformancePoint> {
    let start = items.len().saturating_sub(32);
    items[start..]
        .iter()
        .filter_map(|point| {
            let ts = i64_field(point, &["ts"])?;
            let success_rate = percentage_field(point, &["success_rate", "successRate"])?;
            Some(ProviderPerformancePoint { ts, success_rate })
        })
        .collect()
}

fn percentage_field(value: &Value, field_names: &[&str]) -> Option<f64> {
    f64_field(value, field_names).filter(|number| (0.0..=100.0).contains(number))
}

fn positive_f64_field(value: &Value, field_names: &[&str]) -> Option<f64> {
    f64_field(value, field_names).filter(|number| *number > 0.0)
}

fn f64_field(value: &Value, field_names: &[&str]) -> Option<f64> {
    field_names.iter().find_map(|field_name| {
        value
            .get(*field_name)
            .and_then(Value::as_f64)
            .filter(|number| number.is_finite())
    })
}

fn i64_field(value: &Value, field_names: &[&str]) -> Option<i64> {
    field_names.iter().find_map(|field_name| {
        let item = value.get(*field_name)?;
        item.as_i64()
            .or_else(|| item.as_u64().and_then(|number| i64::try_from(number).ok()))
    })
}

fn looks_unsupported(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("not found")
        || normalized.contains("not enabled")
        || normalized.contains("not supported")
        || normalized.contains("disabled")
        || message.contains("未开启")
        || message.contains("不支持")
        || message.contains("不存在")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_models_are_parsed_with_recent_success_samples() {
        let loaded = parse_performance_body(
            r#"{
                "success": true,
                "data": {
                    "window_start": 100,
                    "window_end": 200,
                    "models": [
                        {
                            "model_name": "gpt-5",
                            "avg_latency_ms": 1800,
                            "success_rate": 99.25,
                            "avg_tps": 42.5,
                            "recent_success_series": [
                                {"ts": 120, "success_rate": 100},
                                {"ts": 180, "success_rate": 98.5}
                            ]
                        },
                        {
                            "model_name": "no-metrics"
                        }
                    ]
                }
            }"#,
        )
        .expect("summary should parse");
        let model = loaded.models.get("gpt-5").expect("model should be present");
        assert_eq!(model.success_rate, Some(99.25));
        assert_eq!(model.avg_latency_ms, Some(1800.0));
        assert_eq!(model.avg_tps, Some(42.5));
        assert_eq!(model.recent_success.len(), 2);
        assert_eq!(model.recent_success[1].success_rate, 98.5);
        assert!(!loaded.models.contains_key("no-metrics"));
        assert_eq!(loaded.window_start, Some(100));
        assert_eq!(loaded.window_end, Some(200));
    }

    #[test]
    fn top_level_success_series_is_used_when_model_omits_it() {
        let loaded = parse_performance_body(
            r#"{
                "success": true,
                "data": {
                    "models": [
                        {"model_name": "gpt-5", "success_rate": 99.0}
                    ],
                    "recent_success_series": {
                        "gpt-5": [{"ts": 10, "success_rate": 100}]
                    }
                }
            }"#,
        )
        .expect("summary should parse");
        assert_eq!(
            loaded.models["gpt-5"].recent_success,
            [ProviderPerformancePoint {
                ts: 10,
                success_rate: 100.0
            }]
        );
    }

    #[test]
    fn disabled_performance_endpoint_is_reported_as_unsupported() {
        let error =
            parse_performance_body(r#"{"success":false,"message":"perf metrics is not enabled"}"#)
                .expect_err("disabled endpoint must not parse");
        assert!(matches!(error, PerformanceFailure::Unsupported(_)));
    }

    #[test]
    fn invalid_metrics_are_ignored_instead_of_becoming_zeroes() {
        let loaded = parse_performance_body(
            r#"{
                "success": true,
                "data": {
                    "models": [
                        {
                            "model_name": "odd-model",
                            "success_rate": 120,
                            "avg_tps": -1,
                            "avg_latency_ms": 0
                        },
                        {
                            "model_name": "ok-model",
                            "success_rate": 0
                        }
                    ]
                }
            }"#,
        )
        .expect("summary should parse");
        assert!(!loaded.models.contains_key("odd-model"));
        assert_eq!(loaded.models["ok-model"].success_rate, Some(0.0));
    }
}
