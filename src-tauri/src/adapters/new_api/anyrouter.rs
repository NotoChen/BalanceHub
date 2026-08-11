//! AnyRouter 的 NewAPI 方言：签到走 `/api/user/sign_in`，且只认 `session` Cookie。
//!
//! 这里不再包含任何过盾逻辑。AnyRouter 站点常用阿里云 WAF，但那是**站点**的属性
//! 而不是**方言**的属性：盾的检测与求解已经上移到 [`crate::network::shield`]，由
//! 通用发送器在响应命中挑战页时自动处理。本模块因此不需要预热请求，也不需要知道
//! 目标站点有没有盾。

use crate::{
    adapters::transport::ProviderTransport,
    models::{Provider, ProviderCheckInResult},
};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT},
    Method, StatusCode,
};
use serde_json::Value;

use super::http::{build_url, USER_AGENT_VALUE};
use super::response::trim_message;

const DEFAULT_UPSTREAM: &str = "https://anyrouter.top";

pub async fn check_in_provider(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderCheckInResult, String> {
    let upstream = normalize_base_url(Some(&provider.identity.base_url), DEFAULT_UPSTREAM);
    if !super::adapter::is_anyrouter_base_url(&upstream) {
        return Err("当前中转站不是 AnyRouter 地址".to_string());
    }

    let session = normalize_session_value(&provider.auth.session_cookie);
    if session.is_empty() {
        return Err("AnyRouter 签到需要在中转站中配置会话 Cookie".to_string());
    }

    // 直接发签到请求，不预热、不猜站点有没有盾。命中挑战页时通用层会用挑战页
    // 正文本身求解并重试——比原先"每次签到都先探一遍"少一半请求。
    let request = client
        .request(Method::POST, build_url(&upstream, "/api/user/sign_in")?)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, &upstream)
        .header(REFERER, format!("{upstream}/"))
        .header(COOKIE, format!("session={session}"))
        .body("");

    let response = client.send(request, "请求 AnyRouter 签到").await?;
    let status = response.status;
    let body = response.body;
    let result = parse_check_in_response(status, &body);

    Ok(ProviderCheckInResult {
        ok: result.ok,
        message: result.message,
        last_checked_in_at: None,
        last_check_in_user: None,
        quota_delta: None,
    })
}

pub fn normalize_session_cookie(raw: &str) -> String {
    normalize_session_value(raw)
}

struct AccountResult {
    ok: bool,
    message: String,
}

fn parse_check_in_response(response_status: StatusCode, body_text: &str) -> AccountResult {
    if response_status == StatusCode::UNAUTHORIZED {
        return AccountResult {
            ok: false,
            message: format!("认证无效(401): {}", trim_message(body_text)),
        };
    }

    if !response_status.is_success() {
        return AccountResult {
            ok: false,
            message: format!(
                "HTTP {}: {}",
                response_status.as_u16(),
                trim_message(body_text)
            ),
        };
    }

    if !body_text.trim_start().starts_with('{') {
        return AccountResult {
            ok: false,
            message: format!("响应非 JSON: {}", trim_message(body_text)),
        };
    }

    let data = match serde_json::from_str::<Value>(body_text) {
        Ok(data) => data,
        Err(err) => {
            return AccountResult {
                ok: false,
                message: format!("解析 JSON 失败: {err}"),
            };
        }
    };

    let message = data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    match data.get("success").and_then(Value::as_bool) {
        Some(true) => AccountResult {
            ok: true,
            message: if message.is_empty() {
                "今日已签到".to_string()
            } else {
                message.to_string()
            },
        },
        Some(false) => AccountResult {
            ok: anyrouter_message_indicates_already_checked_in(message),
            message: if message.is_empty() {
                format!("签到失败: {data}")
            } else {
                message.to_string()
            },
        },
        None => AccountResult {
            ok: anyrouter_message_indicates_already_checked_in(message),
            message: if message.is_empty() {
                format!("响应缺少 success 字段: {data}")
            } else {
                format!("响应缺少 success 字段: {message}")
            },
        },
    }
}

pub(crate) fn anyrouter_message_indicates_already_checked_in(message: &str) -> bool {
    let compact = message
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    compact.contains("已签到")
        || compact.contains("已经签到")
        || compact.contains("已签过")
        || compact.contains("已经签过")
        || compact.contains("重复签到")
        || (compact.contains("already") && compact.contains("sign"))
        || (compact.contains("already") && compact.contains("check"))
}

fn normalize_base_url(raw: Option<&str>, fallback: &str) -> String {
    let base = raw.unwrap_or(fallback).trim();
    base.trim_end_matches('/').to_string()
}

fn normalize_session_value(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }

    for part in text.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("session") {
            return value.trim().to_string();
        }
    }

    text.to_string()
}

// build_url 统一复用 http 的实现：Url::join 对以 "/" 开头的 path 会
// 整段替换 base 的 path，子路径部署（如 https://host/relay）会被截断到根路径。

#[cfg(test)]
mod tests {
    use super::{
        anyrouter_message_indicates_already_checked_in, normalize_session_value,
        parse_check_in_response,
    };
    use reqwest::StatusCode;

    #[test]
    fn normalizes_session_cookie_value() {
        assert_eq!(normalize_session_value("session=abc123; path=/"), "abc123");
        assert_eq!(normalize_session_value("abc123"), "abc123");
    }

    #[test]
    fn treats_already_checked_in_response_as_ok() {
        let result = parse_check_in_response(
            StatusCode::OK,
            r#"{"success":false,"message":"今日已签到"}"#,
        );

        assert!(result.ok);
        assert_eq!(result.message, "今日已签到");
    }

    #[test]
    fn recognizes_common_already_checked_in_messages() {
        assert!(anyrouter_message_indicates_already_checked_in(
            "今天已经签到过了"
        ));
        assert!(anyrouter_message_indicates_already_checked_in(
            "already signed in"
        ));
        assert!(!anyrouter_message_indicates_already_checked_in(
            "签到失败，余额不足"
        ));
    }

    #[test]
    fn missing_success_is_not_treated_as_success() {
        let result = parse_check_in_response(StatusCode::OK, r#"{"message":"ok"}"#);
        assert!(!result.ok);
        assert!(result.message.contains("响应缺少 success 字段"));
    }

    #[test]
    fn unauthorized_response_keeps_server_detail() {
        let result =
            parse_check_in_response(StatusCode::UNAUTHORIZED, r#"{"message":"session expired"}"#);
        assert!(!result.ok);
        assert!(result.message.contains("session expired"));
    }
}
