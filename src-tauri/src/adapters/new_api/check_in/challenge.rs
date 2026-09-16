use crate::models::{ProviderCheckInResult, ProviderCheckInVerification};
use reqwest::{header::HeaderMap, StatusCode};
use serde_json::Value;

pub(crate) fn verification_required(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> Option<ProviderCheckInVerification> {
    let lower = body.to_lowercase();
    // Rate limiting is not a challenge and must never trigger browser retries.
    if status == StatusCode::TOO_MANY_REQUESTS
        || lower.contains("error 1015")
        || lower.contains("you are being rate limited")
    {
        return None;
    }
    if headers
        .get("cf-mitigated")
        .is_some_and(|v| v == "challenge")
        || ((lower.contains("<html") || lower.contains("<!doctype html"))
            && (lower.contains("/cdn-cgi/challenge-platform/") || lower.contains("_cf_chl_opt")))
    {
        return Some(ProviderCheckInVerification::Cloudflare);
    }
    let value = serde_json::from_str::<Value>(body).ok()?;
    if value.get("success").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let message = value
        .get("message")
        .or_else(|| value.get("msg"))
        .and_then(Value::as_str)?
        .to_lowercase();
    (message.contains("turnstile") || message.contains("人机验证") || message.contains("人机校验"))
        .then_some(ProviderCheckInVerification::Turnstile)
}

pub(crate) fn verification_result(kind: ProviderCheckInVerification) -> ProviderCheckInResult {
    ProviderCheckInResult {
        ok: false,
        message: "正在转入浏览器完成签到验证".to_string(),
        verification_required: Some(kind),
        unconfirmed: false,
        last_checked_in_at: None,
        last_check_in_user: None,
        quota_delta: None,
    }
}

pub(crate) fn parse_checked_in(status: StatusCode, body: &str) -> Result<bool, String> {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err("站点请求过于频繁，请稍后再签到".to_string());
    }
    if !status.is_success() {
        return Err(format!("读取签到状态失败：HTTP {}", status.as_u16()));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "站点没有返回有效的签到状态，已停止提交".to_string())?;
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let message = value
            .get("message")
            .or_else(|| value.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or("签到状态读取失败");
        return Err(super::super::response::trim_message(message));
    }
    value
        .pointer("/data/stats/checked_in_today")
        .or_else(|| value.pointer("/data/checked_in_today"))
        .and_then(Value::as_bool)
        .ok_or_else(|| "站点没有提供今日签到状态，已停止提交".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_detection_requires_actual_rejection() {
        let headers = HeaderMap::new();
        assert_eq!(
            verification_required(
                StatusCode::OK,
                &headers,
                r#"{"success":true,"data":{"turnstile_check":true}}"#,
            ),
            None
        );
        assert_eq!(
            verification_required(
                StatusCode::OK,
                &headers,
                r#"{"success":false,"message":"Turnstile token is empty"}"#,
            ),
            Some(ProviderCheckInVerification::Turnstile)
        );
        assert_eq!(
            verification_required(StatusCode::FORBIDDEN, &headers, "Cloudflare: forbidden"),
            None
        );
        let mut headers = headers;
        headers.insert("cf-mitigated", "challenge".parse().unwrap());
        assert_eq!(
            verification_required(StatusCode::FORBIDDEN, &headers, ""),
            Some(ProviderCheckInVerification::Cloudflare)
        );
        assert_eq!(
            verification_required(StatusCode::TOO_MANY_REQUESTS, &headers, ""),
            None
        );
    }

    #[test]
    fn unknown_or_rejected_status_does_not_authorize_submission() {
        assert!(parse_checked_in(StatusCode::OK, "{}").is_err());
        assert!(parse_checked_in(StatusCode::FORBIDDEN, "{}").is_err());
        assert_eq!(
            parse_checked_in(
                StatusCode::OK,
                r#"{"success":true,"data":{"stats":{"checked_in_today":false}}}"#,
            ),
            Ok(false)
        );
    }
}
