use crate::{
    models::{AppSettings, LivenessHttpProtocol, LivenessRecord, Provider},
    providers::{newapi_http::build_client, newapi_response::trim_message},
};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use super::output::{failure_record, sanitize_output, ParsedTokenUsage};
use super::LivenessContext;

struct HttpOutcome {
    ok: bool,
    response_text: String,
    raw_body: String,
    usage: ParsedTokenUsage,
    message: String,
}

/// 通过一次真实的最小补全请求探活（不使用 /models，那不触发推理、无意义）。
/// 同步入口：内部 `block_on` 驱动异步 reqwest（本函数始终在阻塞线程上运行）。
pub(super) fn run_http_liveness(
    settings: &AppSettings,
    provider: &Provider,
    context: &LivenessContext,
    checked_at: String,
    source: String,
) -> LivenessRecord {
    let client = match build_client(settings, provider) {
        Ok(client) => client,
        Err(err) => {
            return failure_record(
                checked_at,
                source,
                format!("初始化 HTTP 客户端失败: {err}"),
                context.command_preview.clone(),
                context.prompt.clone(),
            );
        }
    };

    let protocol = context.http_protocol;
    let api_key = provider.auth.api_key.trim().to_string();
    let started_at = Instant::now();
    let outcome = tauri::async_runtime::block_on(perform_http_request(
        &client,
        protocol,
        &context.base_url,
        &context.model,
        &context.prompt,
        &api_key,
        Duration::from_secs(context.timeout_seconds.max(1)),
    ));
    let latency_ms = started_at.elapsed().as_millis();

    LivenessRecord {
        checked_at,
        source,
        cli_kind: http_kind_value(protocol),
        ok: outcome.ok,
        latency_ms,
        model: context.model.clone(),
        base_url: context.base_url.clone(),
        prompt: context.prompt.clone(),
        response_preview: outcome.response_text.chars().take(240).collect(),
        response_raw: sanitize_output(&outcome.raw_body),
        input_tokens: outcome.usage.input_tokens,
        cached_input_tokens: outcome.usage.cached_input_tokens,
        output_tokens: outcome.usage.output_tokens,
        reasoning_output_tokens: outcome.usage.reasoning_output_tokens,
        total_tokens: outcome.usage.total_tokens,
        total_cost_usd: outcome.usage.total_cost_usd,
        message: outcome.message,
        command_preview: context.command_preview.clone(),
    }
}

async fn perform_http_request(
    client: &Client,
    protocol: LivenessHttpProtocol,
    base_url: &str,
    model: &str,
    prompt: &str,
    api_key: &str,
    timeout: Duration,
) -> HttpOutcome {
    let (url, body, is_anthropic) = build_request(protocol, base_url, model, prompt);
    let mut request = client.post(&url).timeout(timeout).json(&body);
    if is_anthropic {
        request = request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        request = request.bearer_auth(api_key);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return HttpOutcome {
                ok: false,
                response_text: String::new(),
                raw_body: String::new(),
                usage: ParsedTokenUsage::default(),
                message: format!("测活请求失败（网络异常，不代表账号失效）: {err}"),
            };
        }
    };
    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return HttpOutcome {
            ok: false,
            response_text: String::new(),
            usage: ParsedTokenUsage::default(),
            message: classify_http_failure(status, &body_text),
            raw_body: body_text,
        };
    }

    let value = match serde_json::from_str::<Value>(&body_text) {
        Ok(value) => value,
        Err(err) => {
            return HttpOutcome {
                ok: false,
                response_text: String::new(),
                usage: ParsedTokenUsage::default(),
                message: format!("解析测活响应失败: {err}: {}", trim_message(&body_text)),
                raw_body: body_text,
            };
        }
    };

    let (response_text, usage) = parse_response(protocol, &value);
    let ok = !response_text.trim().is_empty();
    let message = if ok {
        "测活成功".to_string()
    } else {
        let detail = api_error_message(&value);
        if detail.is_empty() {
            format!("测活响应为空: {}", trim_message(&body_text))
        } else {
            detail
        }
    };

    HttpOutcome {
        ok,
        response_text,
        raw_body: body_text,
        usage,
        message,
    }
}

/// 按协议构造 (端点, 请求体, 是否 Anthropic 鉴权)。
fn build_request(
    protocol: LivenessHttpProtocol,
    base_url: &str,
    model: &str,
    prompt: &str,
) -> (String, Value, bool) {
    let base = base_url.trim_end_matches('/');
    match protocol {
        LivenessHttpProtocol::OpenaiChat => (
            format!("{base}/chat/completions"),
            json!({
                "model": model,
                "messages": [{ "role": "user", "content": prompt }],
                "max_tokens": 64,
            }),
            false,
        ),
        LivenessHttpProtocol::OpenaiResponses => (
            format!("{base}/responses"),
            json!({
                "model": model,
                "input": prompt,
                "max_output_tokens": 64,
            }),
            false,
        ),
        LivenessHttpProtocol::Anthropic => (
            format!("{base}/v1/messages"),
            json!({
                "model": model,
                "max_tokens": 64,
                "messages": [{ "role": "user", "content": prompt }],
            }),
            true,
        ),
    }
}

/// 按协议解析出助手文本 + token 用量。
fn parse_response(protocol: LivenessHttpProtocol, value: &Value) -> (String, ParsedTokenUsage) {
    let usage = value.get("usage");
    match protocol {
        LivenessHttpProtocol::OpenaiChat => {
            let text = value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let input = field_u64(usage, "prompt_tokens");
            let output = field_u64(usage, "completion_tokens");
            (
                text,
                ParsedTokenUsage {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: field_u64(usage, "total_tokens").or_else(|| sum(input, output)),
                    ..ParsedTokenUsage::default()
                },
            )
        }
        LivenessHttpProtocol::OpenaiResponses => {
            let text = extract_responses_text(value);
            let input = field_u64(usage, "input_tokens");
            let output = field_u64(usage, "output_tokens");
            (
                text,
                ParsedTokenUsage {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: field_u64(usage, "total_tokens").or_else(|| sum(input, output)),
                    ..ParsedTokenUsage::default()
                },
            )
        }
        LivenessHttpProtocol::Anthropic => {
            let text = value
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<String>()
                })
                .unwrap_or_default()
                .trim()
                .to_string();
            let input = field_u64(usage, "input_tokens");
            let output = field_u64(usage, "output_tokens");
            (
                text,
                ParsedTokenUsage {
                    input_tokens: input,
                    cached_input_tokens: field_u64(usage, "cache_read_input_tokens"),
                    output_tokens: output,
                    total_tokens: sum(input, output),
                    ..ParsedTokenUsage::default()
                },
            )
        }
    }
}

fn extract_responses_text(value: &Value) -> String {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return text.trim().to_string();
    }
    let mut buffer = String::new();
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for block in content {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        buffer.push_str(text);
                    }
                }
            }
        }
    }
    buffer.trim().to_string()
}

fn field_u64(usage: Option<&Value>, key: &str) -> Option<u64> {
    usage
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
}

fn sum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        _ => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

fn api_error_message(value: &Value) -> String {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// 区分「账号/凭据问题」与「瞬时故障」：自动测活的价值在于判断账号死活，
/// 限流和站点 5xx 不该被读成死号。
fn classify_http_failure(status: reqwest::StatusCode, body: &str) -> String {
    let code = status.as_u16();
    let detail = trim_message(body);
    match code {
        401 | 403 => format!("HTTP {code}: 凭据或权限被拒绝，API Key 可能已失效: {detail}"),
        429 => format!("HTTP {code}: 请求被限流（瞬时问题，不代表账号失效）: {detail}"),
        500..=599 => format!("HTTP {code}: 站点服务异常（不代表账号失效）: {detail}"),
        _ => format!("HTTP {code}: {detail}"),
    }
}

fn http_kind_value(protocol: LivenessHttpProtocol) -> String {
    match protocol {
        LivenessHttpProtocol::OpenaiChat => "httpOpenaiChat",
        LivenessHttpProtocol::OpenaiResponses => "httpOpenaiResponses",
        LivenessHttpProtocol::Anthropic => "httpAnthropic",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn classifies_credential_failures_and_transient_failures() {
        assert!(
            classify_http_failure(StatusCode::UNAUTHORIZED, "bad key").contains("凭据或权限被拒绝")
        );
        assert!(classify_http_failure(StatusCode::FORBIDDEN, "").contains("凭据或权限被拒绝"));
        assert!(classify_http_failure(StatusCode::TOO_MANY_REQUESTS, "").contains("不代表账号失效"));
        assert!(classify_http_failure(StatusCode::BAD_GATEWAY, "").contains("不代表账号失效"));
        assert!(classify_http_failure(StatusCode::NOT_FOUND, "no route").starts_with("HTTP 404"));
    }
}
