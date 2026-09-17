use crate::models::{
    check_in_message_indicates_disabled, AuthMode, Provider, ProviderCheckInRecordsResult,
    ProviderCheckInResult, ProviderQuotaDisplay,
};
use crate::util::current_month;
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT},
    StatusCode,
};
use serde::Deserialize;
use serde_json::Value;

#[path = "check_in/challenge.rs"]
mod challenge;
#[path = "check_in/executor.rs"]
mod executor;
#[path = "check_in/fresh_login.rs"]
mod fresh_login;
#[path = "check_in/records.rs"]
mod records;
#[path = "check_in/session_sign_in.rs"]
mod session_sign_in;
#[path = "check_in/standard.rs"]
mod standard;
#[cfg(test)]
#[path = "check_in/tests.rs"]
mod tests;

use crate::{
    adapters::{browser::BrowserSession, protocol::contracts::ProviderOperationOutcome},
    models::{
        provider_domain::check_in as policy, CheckInError, CheckInVerificationRequest,
        ProviderCheckInMethod, ProviderCheckInVerification, ProviderTurnstileMode,
    },
};
use executor::Executor;
pub(super) use session_sign_in::normalize_session_cookie;

use super::http::{
    apply_auth_headers, apply_session_cookie, build_url, normalize_base_url, ProviderTransport,
    USER_AGENT_VALUE,
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
) -> Result<(Provider, ProviderCheckInResult), String> {
    let mut effective = provider.clone();
    let result = execute(&mut Executor::Http(client), &mut effective, None).await;
    let result = finish_result(result).map_err(|error| error.to_string())?;
    Ok((effective, result))
}

pub(crate) async fn check_in_with_browser(
    session: &mut BrowserSession,
    provider: &Provider,
    verification: CheckInVerificationRequest,
    ensure_current: &(dyn Fn() -> Result<(), String> + Send + Sync),
) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, CheckInError> {
    let mut effective = provider.clone();
    let mut executor = Executor::Browser {
        session,
        ensure_current,
    };
    let result = finish_result(execute(&mut executor, &mut effective, Some(verification)).await)?;
    Ok(ProviderOperationOutcome::authenticated(
        provider, effective, result,
    ))
}

async fn execute(
    executor: &mut Executor<'_>,
    provider: &mut Provider,
    mut verification: Option<CheckInVerificationRequest>,
) -> Result<ProviderCheckInResult, CheckInError> {
    policy::validate_credentials(provider)?;
    let method = policy::effective_method(provider);
    let always = provider.automation.turnstile_mode == ProviderTurnstileMode::Always;
    if method == ProviderCheckInMethod::FreshLogin {
        return fresh_login::run(
            executor,
            provider,
            always
                || verification
                    .is_some_and(|request| request.kind == ProviderCheckInVerification::Turnstile),
        )
        .await;
    }
    // Preserve the HTTP password-login preflight. Browser continuation reuses
    // that freshly persisted session instead of repeating the login mutation.
    if provider.auth.mode == AuthMode::Password
        && (!executor.is_browser()
            || verification.is_some_and(|request| request.requires_login)
            || provider.auth.session_cookie.trim().is_empty()
            || method == ProviderCheckInMethod::Standard
                && provider.auth.api_user.trim().is_empty())
    {
        if let Some(result) = fresh_login::login(
            executor,
            provider,
            verification
                .is_some_and(|request| request.kind == ProviderCheckInVerification::Turnstile),
        )
        .await?
        {
            return Ok(result);
        }
        verification = None;
    }
    let needs_token = always
        || verification
            .is_some_and(|request| request.kind == ProviderCheckInVerification::Turnstile);
    match method {
        ProviderCheckInMethod::SessionSignIn => {
            session_sign_in::run(executor, provider, needs_token).await
        }
        _ => standard::run(executor, provider, needs_token).await,
    }
}

fn finish_result(
    result: Result<ProviderCheckInResult, CheckInError>,
) -> Result<ProviderCheckInResult, CheckInError> {
    match result {
        Err(CheckInError::Unconfirmed(message)) => Ok(ProviderCheckInResult {
            ok: false,
            message,
            unconfirmed: true,
            verification_required: None,
            verification_requires_login: false,
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        }),
        other => other,
    }
}

pub(crate) fn browser_cookie_header(provider: &Provider) -> String {
    if policy::effective_method(provider) == ProviderCheckInMethod::SessionSignIn {
        let session = normalize_session_cookie(&provider.auth.session_cookie);
        return if session.is_empty() {
            String::new()
        } else {
            format!("session={session}")
        };
    }
    if matches!(provider.auth.mode, AuthMode::Session | AuthMode::Password) {
        super::http::provider_cookie_header_for_base(
            &provider.auth.session_cookie,
            &provider.identity.base_url,
        )
    } else {
        String::new()
    }
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

fn already_checked_in() -> ProviderCheckInResult {
    ProviderCheckInResult {
        ok: true,
        message: "今日已签到".to_string(),
        verification_required: None,
        verification_requires_login: false,
        unconfirmed: false,
        last_checked_in_at: None,
        last_check_in_user: None,
        quota_delta: None,
    }
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
            verification_required: None,
            verification_requires_login: false,
            unconfirmed: false,
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
                verification_required: None,
                verification_requires_login: false,
                unconfirmed: false,
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
            verification_required: None,
            verification_requires_login: false,
            unconfirmed: false,
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
            verification_required: None,
            verification_requires_login: false,
            unconfirmed: false,
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
