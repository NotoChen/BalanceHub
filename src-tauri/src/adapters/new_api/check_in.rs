use crate::models::{
    check_in_message_indicates_disabled, AuthMode, Provider, ProviderCheckInRecordsResult,
    ProviderCheckInResult, ProviderQuotaDisplay,
};
use crate::util::current_month;
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT},
    Method, StatusCode,
};
use serde::Deserialize;
use serde_json::Value;

#[path = "check_in/records.rs"]
mod records;

use super::http::{
    apply_auth_headers, apply_session_cookie, build_url, normalize_base_url, provider_is_anyrouter,
    ProviderTransport, USER_AGENT_VALUE,
};
use super::response::{parse_success_data, send_text, trim_message};
use super::site::{apply_site_metadata, fetch_site_metadata_or, site_metadata_from_provider};

#[derive(Debug, Deserialize)]
struct SignInResponse {
    success: Option<bool>,
    message: Option<String>,
    msg: Option<String>,
    code: Option<i64>,
    ret: Option<i64>,
}

pub(crate) async fn probe_check_in_capability(
    client: &ProviderTransport,
    provider: &Provider,
    base_url: &str,
) -> Result<Vec<AuthMode>, String> {
    let mut modes = Vec::new();
    let mut errors = Vec::new();

    if !provider.auth.access_token.trim().is_empty() {
        let mut testing_provider = provider.clone();
        testing_provider.auth.mode = AuthMode::AccessToken;
        match check_in_status_probe(client, &testing_provider, base_url).await {
            Ok(true) => modes.push(AuthMode::AccessToken),
            Ok(false) => {}
            Err(message) => errors.push(format!("访问令牌: {message}")),
        }
    }

    if !provider.auth.session_cookie.trim().is_empty() {
        let mut testing_provider = provider.clone();
        testing_provider.auth.mode = AuthMode::Session;
        match check_in_status_probe(client, &testing_provider, base_url).await {
            Ok(true) => modes.push(AuthMode::Session),
            Ok(false) => {}
            Err(message) => errors.push(format!("会话 Cookie: {message}")),
        }
    }

    if !modes.is_empty() || errors.is_empty() {
        return Ok(modes);
    }

    Err(errors.join("；"))
}

pub async fn check_in_provider(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderCheckInResult, String> {
    if provider_is_anyrouter(provider) {
        return Err("AnyRouter 签到需要走专用逻辑".to_string());
    }

    check_in_provider_once(client, provider).await
}

pub async fn fetch_check_in_records(
    client: &ProviderTransport,
    provider: &Provider,
    month: &str,
) -> Result<ProviderCheckInRecordsResult, String> {
    fetch_check_in_records_once(client, provider, month).await
}

async fn fetch_check_in_records_once(
    client: &ProviderTransport,
    provider: &Provider,
    month: &str,
) -> Result<ProviderCheckInRecordsResult, String> {
    let month = records::normalize_month(month)?;
    validate_check_in_credentials(provider)?;

    let base_url = normalize_base_url(&provider.identity.base_url);
    let url = build_url(&base_url, &format!("/api/user/checkin?month={month}"))?;
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, &base_url)
        .header(REFERER, format!("{base_url}/"));

    request = apply_auth_headers(request, provider);
    request = apply_session_cookie(request, provider);

    let (status, body) = send_text(client, request, "读取签到记录").await?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), trim_message(&body)));
    }
    if check_in_message_indicates_disabled(&trim_message(&body)) {
        return Err("当前站点未开启签到".to_string());
    }

    let data = parse_success_data(&status, body, "签到记录")?;
    let site =
        fetch_site_metadata_or(client, &base_url, site_metadata_from_provider(provider)).await?;
    let mut quota_provider = provider.clone();
    apply_site_metadata(&mut quota_provider, site.clone());
    let quota_display = ProviderQuotaDisplay {
        quota_display_type: site.quota_display_type.clone(),
        currency_symbol: site.currency_symbol.clone(),
    };
    let records = records::collect(&data, &month, &quota_provider);

    Ok(ProviderCheckInRecordsResult {
        provider_id: provider.identity.id.clone(),
        month,
        message: if records.is_empty() {
            "当前月份没有可展示的签到记录".to_string()
        } else {
            format!("已获取 {} 条签到记录", records.len())
        },
        records,
        quota_display,
    })
}

async fn check_in_status_probe(
    client: &ProviderTransport,
    provider: &Provider,
    base_url: &str,
) -> Result<bool, String> {
    let url = build_url(
        base_url,
        &format!("/api/user/checkin?month={}", current_month()),
    )?;
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"));

    request = apply_auth_headers(request, provider);
    request = apply_session_cookie(request, provider);

    let (status, body) = send_text(client, request, "探测签到能力").await?;
    if status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED {
        return Ok(false);
    }
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), trim_message(&body)));
    }
    serde_json::from_str::<Value>(&body)
        .map_err(|err| format!("解析签到状态失败: {err}: {}", trim_message(&body)))?;
    if check_in_message_indicates_disabled(&trim_message(&body)) {
        return Ok(false);
    }
    Ok(true)
}

async fn check_in_provider_once(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderCheckInResult, String> {
    validate_check_in_credentials(provider)?;

    let base_url = normalize_base_url(&provider.identity.base_url);
    if check_in_status(client, provider, &base_url).await? {
        return Ok(ProviderCheckInResult {
            ok: true,
            message: "今日已签到".to_string(),
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        });
    }

    let url = build_url(&base_url, "/api/user/checkin")?;
    let mut request = client
        .request(Method::POST, url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, &base_url)
        .header(REFERER, format!("{base_url}/"))
        .header("X-Requested-With", "XMLHttpRequest")
        .body("");

    request = apply_auth_headers(request, provider);
    request = apply_session_cookie(request, provider);

    let response = client.send(request, "请求签到").await?;
    let status = response.status;
    let body = response.body;

    Ok(parse_check_in_response(status, &body))
}

async fn check_in_status(
    client: &ProviderTransport,
    provider: &Provider,
    base_url: &str,
) -> Result<bool, String> {
    let url = build_url(
        base_url,
        &format!("/api/user/checkin?month={}", current_month()),
    )?;
    let mut request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"));

    request = apply_auth_headers(request, provider);
    request = apply_session_cookie(request, provider);

    let (status, body) = send_text(client, request, "读取签到状态").await?;

    if !status.is_success() {
        return Ok(false);
    }

    let decoded = match serde_json::from_str::<Value>(&body) {
        Ok(decoded) => decoded,
        Err(_) => return Ok(false),
    };

    Ok(decoded
        .pointer("/data/stats/checked_in_today")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn validate_check_in_credentials(provider: &Provider) -> Result<(), String> {
    match provider.auth.mode {
        AuthMode::AccessToken
            if provider.auth.access_token.trim().is_empty()
                || provider.auth.api_user.trim().is_empty() =>
        {
            Err("AccessToken 签到需要访问令牌和 API User ID".to_string())
        }
        AuthMode::Session
            if provider.auth.session_cookie.trim().is_empty()
                || provider.auth.api_user.trim().is_empty() =>
        {
            Err("Cookie 签到需要会话 Cookie 和 API User ID".to_string())
        }
        AuthMode::ApiKey => Err("API 密钥不支持用户签到，请改用访问令牌或会话 Cookie".to_string()),
        AuthMode::Password => Err("账号密码尚未完成登录，请先测试连接或刷新余额".to_string()),
        _ => Ok(()),
    }
}

fn parse_check_in_response(status: StatusCode, body: &str) -> ProviderCheckInResult {
    if !status.is_success() && status != StatusCode::BAD_REQUEST {
        return ProviderCheckInResult {
            ok: false,
            message: format!("HTTP {}: {}", status.as_u16(), trim_message(body)),
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        };
    }

    let decoded = match serde_json::from_str::<SignInResponse>(body) {
        Ok(decoded) => decoded,
        Err(err) => {
            return ProviderCheckInResult {
                ok: false,
                message: format!("解析签到响应失败: {err}: {}", trim_message(body)),
                last_checked_in_at: None,
                last_check_in_user: None,
                quota_delta: None,
            };
        }
    };

    let message = decoded.message.or(decoded.msg).unwrap_or_default();
    let ok = decoded.success.unwrap_or(false)
        || decoded.ret == Some(1)
        || decoded.code == Some(0)
        || message.contains("已经签到")
        || message.contains("已签到")
        || message.contains("签到成功");

    if ok {
        ProviderCheckInResult {
            ok: true,
            message: if message.trim().is_empty() {
                "签到成功".to_string()
            } else {
                message
            },
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        }
    } else {
        ProviderCheckInResult {
            ok: false,
            message: if message.trim().is_empty() {
                format!("签到失败: {}", trim_message(body))
            } else {
                message
            },
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        }
    }
}
