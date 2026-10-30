use crate::{
    models::{AppSettings, Provider},
    network,
};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;

pub(crate) async fn client(settings: &AppSettings, provider: &Provider) -> Result<Client, String> {
    let settings = settings.clone();
    let provider = provider.clone();
    tauri::async_runtime::spawn_blocking(move || {
        network::build_provider_client_with_proxy(
            &settings,
            network::resolve_proxy(&settings, &provider),
        )
    })
    .await
    .map_err(|_| "初始化账号连接失败")?
}

pub(crate) async fn bytes(request: RequestBuilder, context: &str) -> Result<Vec<u8>, String> {
    let mut response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            format!("{context}超时，请稍后重试")
        } else {
            format!("{context}连接失败，请检查网络与代理")
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(http_error(context, status.as_u16()));
    }
    if response
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v != "0")
    {
        return Err(format!("{context}返回 gRPC 错误"));
    }
    const MAX: usize = 4 * 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|len| len > MAX as u64)
    {
        return Err(format!("{context}响应过大"));
    }
    let mut result = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| format!("{context}响应读取失败"))?
    {
        if result.len().saturating_add(chunk.len()) > MAX {
            return Err(format!("{context}响应过大"));
        }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}

pub(crate) async fn json(request: RequestBuilder, context: &str) -> Result<Value, String> {
    serde_json::from_slice(&bytes(request, context).await?)
        .map_err(|_| format!("{context}响应格式无效"))
}

pub(crate) fn http_error(context: &str, status: u16) -> String {
    match status {
        401 => format!("{context}认证已失效（HTTP 401），请重新授权"),
        403 => format!("{context}被拒绝（HTTP 403），请检查账号权限或地区限制"),
        429 => format!("{context}请求过于频繁（HTTP 429），请稍后重试"),
        _ => format!("{context}失败（HTTP {status}）"),
    }
}

pub(crate) fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|v| v.is_finite())
}
pub(crate) fn timestamp(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse().ok())
        .map(|n| if n > 10_000_000_000 { n / 1000 } else { n })
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(value.as_str()?)
                .ok()
                .map(|v| v.timestamp())
        })
        .filter(|at| *at >= 0 && chrono::DateTime::from_timestamp(*at, 0).is_some())
}
pub(crate) fn text(value: &Value, names: &[&str]) -> String {
    names
        .iter()
        .find_map(|name| {
            value
                .get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_default()
        .to_owned()
}
