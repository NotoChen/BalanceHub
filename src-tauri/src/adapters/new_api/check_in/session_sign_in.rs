//! Session-only `/api/user/sign_in`, selectable on any compatible NewAPI site.
use crate::models::{
    CheckInError, CheckInPhase, Provider, ProviderCheckInResult, ProviderCheckInVerification,
};
use reqwest::{Method, StatusCode};
use serde_json::Value;

use super::{
    super::{http::build_url, response::trim_message},
    challenge::{verification_required, verification_result},
    executor::{Credentials, Executor},
};

pub(super) async fn run(
    executor: &mut Executor<'_>,
    provider: &Provider,
    mut needs_token: bool,
) -> Result<ProviderCheckInResult, CheckInError> {
    if normalize_session_cookie(&provider.auth.session_cookie).is_empty() {
        return Err("会话签到需要 session Cookie".into());
    }
    let public_url = build_url(&provider.identity.base_url, "/api/status")?;
    for attempt in 0..3 {
        let mut submit_url = build_url(&provider.identity.base_url, "/api/user/sign_in")?;
        if needs_token {
            let Some(token) = executor.turnstile_token(provider).await? else {
                return Ok(verification_result(ProviderCheckInVerification::Turnstile));
            };
            submit_url
                .query_pairs_mut()
                .append_pair("turnstile", &token);
        }
        executor.phase(CheckInPhase::Requesting);
        let response = executor
            .send(
                provider,
                &submit_url,
                Method::POST,
                Credentials::SessionOnly,
                Some(String::new()),
            )
            .await
            .map_err(|_| {
                CheckInError::Unconfirmed(
                    "会话签到提交后连接中断；已停止自动重试，请查看站点记录".into(),
                )
            })?;
        if let Some(kind) =
            verification_required(response.status, &response.headers, &response.body)
        {
            if !executor
                .retry_verification(provider, kind, &public_url)
                .await?
            {
                return Ok(verification_result(kind));
            }
            if attempt == 2 {
                return Err("完成验证后站点仍拒绝签到，已停止重试".into());
            }
            needs_token |= kind == ProviderCheckInVerification::Turnstile;
            continue;
        }
        let result = parse_check_in_response(response.status, &response.body);
        let explicitly_rejected = serde_json::from_str::<Value>(&response.body)
            .ok()
            .and_then(|value| value.get("success").and_then(Value::as_bool))
            == Some(false);
        if response.status.is_success() && !result.ok && !explicitly_rejected {
            return Err(CheckInError::Unconfirmed(
                "会话签到请求已返回，但结果无法确认；已停止自动重试，请查看站点记录".into(),
            ));
        }
        return Ok(ProviderCheckInResult {
            verification_required: None,
            verification_requires_login: false,
            unconfirmed: false,
            ok: result.ok,
            message: result.message,
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        });
    }
    Err("签到验证未完成".into())
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
