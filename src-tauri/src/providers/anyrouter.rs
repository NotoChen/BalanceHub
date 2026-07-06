use crate::models::{Provider, ProviderCheckInResult};
use reqwest::{
    header::{
        HeaderMap, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE,
        USER_AGENT,
    },
    Client, Method, StatusCode, Url,
};
use serde_json::Value;
use std::collections::BTreeMap;

use super::newapi_response::trim_message;

const DEFAULT_UPSTREAM: &str = "https://anyrouter.top";
const USER_AGENT_VALUE: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/131.0.0.0 Safari/537.36"
);
const XOR_KEY: &str = "3000176000856006061501533003690027800375";
const UNSBOX_TABLE: [usize; 40] = [
    0xf, 0x23, 0x1d, 0x18, 0x21, 0x10, 0x1, 0x26, 0xa, 0x9, 0x13, 0x1f, 0x28, 0x1b, 0x16, 0x17,
    0x19, 0xd, 0x6, 0xb, 0x27, 0x12, 0x14, 0x8, 0xe, 0x15, 0x20, 0x1a, 0x2, 0x1e, 0x7, 0x4, 0x11,
    0x5, 0x3, 0x1c, 0x22, 0x25, 0xc, 0x24,
];

pub async fn check_in_provider(
    client: &Client,
    provider: &Provider,
) -> Result<ProviderCheckInResult, String> {
    let upstream = normalize_base_url(Some(&provider.identity.base_url), DEFAULT_UPSTREAM);
    if !super::newapi::is_anyrouter_base_url(&upstream) {
        return Err("当前中转站不是 AnyRouter 地址".to_string());
    }

    let check_in_url = build_url(&upstream, "/api/user/sign_in")?;
    let challenge_cookie_header =
        get_challenge_cookie_header(client, &upstream, &["/api/user/sign_in", "/api/user/self"])
            .await?;
    let account_result = check_in_provider_one(
        client,
        &check_in_url,
        &upstream,
        &challenge_cookie_header,
        provider,
    )
    .await;

    Ok(ProviderCheckInResult {
        ok: account_result.ok,
        message: account_result.message,
        last_checked_in_at: None,
        last_check_in_user: None,
    })
}

pub async fn challenge_cookie_header(client: &Client, upstream: &str) -> Result<String, String> {
    get_challenge_cookie_header(client, upstream, &["/api/user/self"]).await
}

pub fn normalize_session_cookie(raw: &str) -> String {
    normalize_session_value(raw)
}

struct AccountResult {
    ok: bool,
    message: String,
}

async fn check_in_provider_one(
    client: &Client,
    check_in_url: &Url,
    upstream: &str,
    challenge_cookie_header: &str,
    provider: &Provider,
) -> AccountResult {
    let mut response = match post_check_in_with_provider(
        client,
        check_in_url,
        upstream,
        challenge_cookie_header,
        provider,
    )
    .await
    {
        Ok(response) => response,
        Err(message) => {
            return AccountResult { ok: false, message };
        }
    };

    let mut response_status = response.status();
    if response_status == StatusCode::UNAUTHORIZED {
        return AccountResult {
            ok: false,
            message: "认证无效(401)".to_string(),
        };
    }

    let response_headers = response.headers().clone();
    let mut body_text = match response.text().await {
        Ok(text) => text,
        Err(err) => {
            return AccountResult {
                ok: false,
                message: format!("读取响应失败: {err}"),
            };
        }
    };

    if extract_arg1(&body_text).is_some() {
        let retry_cookie_header =
            build_challenge_cookie_header(&response_headers, &body_text).unwrap_or_default();
        let merged_cookie_header =
            merge_cookie_headers(&[challenge_cookie_header, retry_cookie_header.as_str()]);

        response = match post_check_in_with_provider(
            client,
            check_in_url,
            upstream,
            &merged_cookie_header,
            provider,
        )
        .await
        {
            Ok(response) => response,
            Err(message) => {
                return AccountResult {
                    ok: false,
                    message: format!("重试失败: {message}"),
                };
            }
        };

        response_status = response.status();
        if response_status == StatusCode::UNAUTHORIZED {
            return AccountResult {
                ok: false,
                message: "认证无效(401)".to_string(),
            };
        }

        body_text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                return AccountResult {
                    ok: false,
                    message: format!("读取重试响应失败: {err}"),
                };
            }
        };
    }

    parse_check_in_response(response_status, &body_text)
}

fn parse_check_in_response(response_status: StatusCode, body_text: &str) -> AccountResult {
    if extract_arg1(body_text).is_some() {
        return AccountResult {
            ok: false,
            message: "命中站点验证页，自动重试 1 次仍未通过".to_string(),
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
            ok: true,
            message: format!("返回: {data}"),
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

async fn post_check_in_with_provider(
    client: &Client,
    check_in_url: &Url,
    upstream: &str,
    challenge_cookie_header: &str,
    provider: &Provider,
) -> Result<reqwest::Response, String> {
    let session = normalize_session_value(&provider.auth.session_cookie);
    if session.is_empty() {
        return Err("AnyRouter 签到需要在中转站中配置会话 Cookie".to_string());
    }

    let session_cookie = format!("session={session}");
    let cookie_header = merge_cookie_headers(&[challenge_cookie_header, session_cookie.as_str()]);

    client
        .request(Method::POST, check_in_url.clone())
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, upstream)
        .header(REFERER, format!("{upstream}/"))
        .header(COOKIE, cookie_header)
        .body("")
        .send()
        .await
        .map_err(|err| format!("请求异常: {err}"))
}

async fn get_challenge_cookie_header(
    client: &Client,
    upstream: &str,
    candidate_paths: &[&str],
) -> Result<String, String> {
    for path in candidate_paths {
        let target_url = build_url(upstream, path)?;
        let response = match client
            .get(target_url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => continue,
        };

        let headers = response.headers().clone();
        let body_text = match response.text().await {
            Ok(text) => text,
            Err(_) => continue,
        };

        if let Some(cookie_header) = build_challenge_cookie_header(&headers, &body_text) {
            return Ok(cookie_header);
        }
    }

    Err("获取动态 Cookie 失败: arg1 not found / request failed".to_string())
}

fn build_challenge_cookie_header(headers: &HeaderMap, body_text: &str) -> Option<String> {
    let base_cookies = extract_cookie_pairs(headers);
    let computed_cookie = extract_arg1(body_text).and_then(|arg1| compute_acw_cookie(&arg1).ok());
    if base_cookies.is_empty() && computed_cookie.is_none() {
        return None;
    }

    let mut cookie_sources = base_cookies.iter().map(String::as_str).collect::<Vec<_>>();
    if let Some(cookie) = computed_cookie.as_deref() {
        cookie_sources.push(cookie);
    }

    Some(merge_cookie_headers(&cookie_sources))
}

fn compute_acw_cookie(arg1: &str) -> Result<String, String> {
    if arg1.len() != 40 || !arg1.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("arg1 格式无效".to_string());
    }

    let arg1_chars = arg1.chars().collect::<Vec<_>>();
    let unsboxed = UNSBOX_TABLE
        .iter()
        .map(|index| arg1_chars[index - 1])
        .collect::<String>();

    let mut out = String::with_capacity(40);
    for index in (0..40).step_by(2) {
        let a = u8::from_str_radix(&unsboxed[index..index + 2], 16)
            .map_err(|err| format!("arg1 解析失败: {err}"))?;
        let b = u8::from_str_radix(&XOR_KEY[index..index + 2], 16)
            .map_err(|err| format!("XOR key 解析失败: {err}"))?;
        out.push_str(&format!("{:02x}", a ^ b));
    }

    Ok(format!("acw_sc__v2={out}"))
}

fn extract_arg1(html: &str) -> Option<String> {
    let marker = "var arg1";
    let marker_index = html.find(marker)?;
    let after_marker = &html[marker_index + marker.len()..];
    let equals_index = after_marker.find('=')?;
    let after_equals = after_marker[equals_index + 1..].trim_start();
    let quote = after_equals.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }

    let after_quote = &after_equals[quote.len_utf8()..];
    let end_index = after_quote.find(quote)?;
    let value = &after_quote[..end_index];
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(value.to_string())
    } else {
        None
    }
}

fn extract_cookie_pairs(headers: &HeaderMap) -> Vec<String> {
    let mut pairs = BTreeMap::new();
    for value in headers.get_all(SET_COOKIE) {
        let Ok(line) = value.to_str() else {
            continue;
        };
        let Some((name, value)) = parse_set_cookie_pair(line) else {
            continue;
        };
        pairs.insert(name.to_string(), format!("{name}={value}"));
    }

    pairs.into_values().collect()
}

fn parse_set_cookie_pair(line: &str) -> Option<(&str, &str)> {
    let first_part = line.split(';').next()?.trim();
    let (name, value) = first_part.split_once('=')?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() {
        None
    } else {
        Some((name, value))
    }
}

fn merge_cookie_headers(cookie_sources: &[&str]) -> String {
    let mut pairs = BTreeMap::new();

    for source in cookie_sources {
        for item in source
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let Some((name, value)) = item.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() {
                continue;
            }
            pairs.insert(name.to_string(), format!("{name}={value}"));
        }
    }

    pairs.into_values().collect::<Vec<_>>().join("; ")
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

fn build_url(upstream: &str, path: &str) -> Result<Url, String> {
    let base = Url::parse(upstream).map_err(|err| format!("AnyRouter 地址无效: {err}"))?;
    base.join(path)
        .map_err(|err| format!("AnyRouter 中转站地址无效: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{
        anyrouter_message_indicates_already_checked_in, compute_acw_cookie, extract_arg1,
        merge_cookie_headers, normalize_session_value, parse_check_in_response,
    };
    use reqwest::StatusCode;

    #[test]
    fn extracts_arg1_from_challenge_script() {
        let html = "hello <script>var arg1='0123456789abcdef0123456789abcdef01234567';</script>";
        assert_eq!(
            extract_arg1(html).as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }

    #[test]
    fn normalizes_session_cookie_value() {
        assert_eq!(normalize_session_value("session=abc123; path=/"), "abc123");
        assert_eq!(normalize_session_value("abc123"), "abc123");
    }

    #[test]
    fn merges_cookies_by_name() {
        let merged = merge_cookie_headers(&["a=1; b=1", "b=2; c=3"]);
        assert_eq!(merged, "a=1; b=2; c=3");
    }

    #[test]
    fn computes_acw_cookie_shape() {
        let cookie = compute_acw_cookie("0123456789abcdef0123456789abcdef01234567").unwrap();
        assert!(cookie.starts_with("acw_sc__v2="));
        assert_eq!(cookie.len(), "acw_sc__v2=".len() + 40);
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
}
