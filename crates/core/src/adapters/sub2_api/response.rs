use crate::adapters::transport::ProviderTransport;
use reqwest::{Method, StatusCode, Url};
use serde_json::Value;
use std::time::Duration;

use super::json::string_field;

const API_PREFIX: &str = "/api/v1";

#[derive(Clone)]
pub(super) enum Credential {
    Jwt(String),
}

impl Credential {
    fn value(&self) -> &str {
        match self {
            Self::Jwt(value) => value.trim(),
        }
    }
}

pub(super) async fn request_json(
    client: &ProviderTransport,
    method: Method,
    url: Url,
    credential: Option<Credential>,
    body: Option<Value>,
    context: &str,
) -> Result<Value, String> {
    request_json_inner(client, method, url, credential, body, context, None).await
}

pub(super) async fn request_json_with_timeout(
    client: &ProviderTransport,
    method: Method,
    url: Url,
    credential: Option<Credential>,
    body: Option<Value>,
    context: &str,
    timeout: Duration,
) -> Result<Value, String> {
    request_json_inner(
        client,
        method,
        url,
        credential,
        body,
        context,
        Some(timeout),
    )
    .await
}

async fn request_json_inner(
    client: &ProviderTransport,
    method: Method,
    url: Url,
    credential: Option<Credential>,
    body: Option<Value>,
    context: &str,
    timeout: Option<Duration>,
) -> Result<Value, String> {
    let mut request = client
        .request(method, url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json");
    if let Some(timeout) = timeout {
        request = request.timeout(timeout);
    }
    if let Some(credential) = credential {
        request = request.bearer_auth(credential.value());
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = client.send(request, context).await?;
    parse_response(response.status, &response.body, context)
}

pub(super) fn parse_response(
    status: StatusCode,
    body: &str,
    context: &str,
) -> Result<Value, String> {
    let value = serde_json::from_str::<Value>(body)
        .map_err(|err| format!("解析{context}响应失败: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "HTTP {}: {}",
            status.as_u16(),
            string_field(&value, &["message", "error"]).unwrap_or_else(|| "请求失败".to_string())
        ));
    }
    let code = value
        .get("code")
        .and_then(|item| item.as_i64().or_else(|| item.as_str()?.parse().ok()));
    if code.is_some_and(|code| code != 0)
        || value.get("success").and_then(Value::as_bool) == Some(false)
    {
        return Err(string_field(&value, &["message", "error"])
            .unwrap_or_else(|| "接口返回失败".to_string()));
    }
    Ok(value.get("data").cloned().unwrap_or(value))
}

pub(super) fn api_url(base_url: &str, path: &str) -> Result<Url, String> {
    Url::parse(&format!(
        "{}{API_PREFIX}{}",
        normalize_base_url(base_url),
        path
    ))
    .map_err(|err| format!("Sub2API 地址无效: {err}"))
}

pub(super) fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_unwraps_standard_envelope() {
        let value = parse_response(
            StatusCode::OK,
            r#"{"code":0,"message":"success","data":{"id":7}}"#,
            "测试",
        )
        .expect("response should parse");
        assert_eq!(value["id"], 7);
    }

    #[test]
    fn parse_response_accepts_string_code_and_reports_http_errors() {
        let value = parse_response(StatusCode::OK, r#"{"code":"0","data":[]}"#, "测试")
            .expect("string success code should parse");
        assert!(value.is_array());

        let error = parse_response(
            StatusCode::UNAUTHORIZED,
            r#"{"message":"token expired"}"#,
            "测试",
        )
        .expect_err("401 should fail");
        assert!(error.starts_with("HTTP 401"));
        assert!(error.contains("token expired"));
    }
}
