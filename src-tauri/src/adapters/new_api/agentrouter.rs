//! AgentRouter rewards a fresh login; it has no standard NewAPI check-in API.
use super::{
    check_in::{turnstile_token, verification_required, verification_result},
    http::{
        apply_auth_headers, apply_session_cookie, auth_header_values,
        authenticated_from_login_response, build_url, normalize_base_url, ProviderTransport,
        USER_AGENT_VALUE,
    },
    response::parse_success_data,
};
use crate::{
    adapters::{
        browser::BrowserSession, protocol::contracts::ProviderOperationOutcome,
        transport::TransportResponse,
    },
    models::{
        AuthMode, CheckInError, CheckInPhase, Provider, ProviderCheckInResult,
        ProviderCheckInVerification,
    },
};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT},
    StatusCode,
};
use serde_json::{json, Value};

fn validate(provider: &Provider) -> Result<(), String> {
    if !matches!(provider.auth.mode, AuthMode::Password)
        || provider.auth.login_username.trim().is_empty()
        || provider.auth.login_password.trim().is_empty()
    {
        return Err("AgentRouter 通过重新登录签到，请在中转站配置中填写账号密码".to_string());
    }
    Ok(())
}

fn confirmed() -> ProviderCheckInResult {
    ProviderCheckInResult {
        ok: true,
        message: "已重新登录并确认账号状态（登录签到）".to_string(),
        unconfirmed: false,
        verification_required: None,
        last_checked_in_at: None,
        last_check_in_user: None,
        quota_delta: None,
    }
}

fn unconfirmed(message: &str) -> ProviderCheckInResult {
    let mut result = confirmed();
    result.ok = false;
    result.unconfirmed = true;
    result.message = message.to_string();
    result
}

fn verify_account(response: &TransportResponse, provider: &Provider) -> Result<(), String> {
    let data = parse_success_data(&response.status, response.body.clone(), "确认登录签到")?;
    let id = data.get("id").and_then(|id| {
        id.as_str()
            .map(str::to_string)
            .or_else(|| id.as_i64().map(|id| id.to_string()))
    });
    if id.as_deref() != Some(provider.auth.api_user.as_str()) {
        return Err("登录后账号状态与本次账号不一致".to_string());
    }
    Ok(())
}

pub(super) async fn check_in(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<(Provider, ProviderCheckInResult), String> {
    validate(provider)?;
    let base = normalize_base_url(&provider.identity.base_url);
    let request = client.post(build_url(&base, "/api/user/login")?)
        .header(USER_AGENT, USER_AGENT_VALUE).header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json").header(ORIGIN, &base).header(REFERER, format!("{base}/"))
        .json(&json!({ "username": provider.auth.login_username.trim(), "password": provider.auth.login_password }));
    let response = match client.send(request, "登录签到").await {
        Ok(response) => response,
        Err(_) => {
            return Ok((
                provider.clone(),
                unconfirmed("登录请求后连接中断，结果尚未确认；已停止自动重试"),
            ))
        }
    };
    if let Some(kind) = verification_required(response.status, &response.headers, &response.body) {
        return Ok((provider.clone(), verification_result(kind)));
    }
    let authenticated = match authenticated_from_login_response(provider, &response) {
        Ok(authenticated) => authenticated,
        Err(message) => {
            let rejected = serde_json::from_str::<Value>(&response.body)
                .ok()
                .and_then(|payload| payload.get("success").and_then(Value::as_bool))
                == Some(false);
            if rejected {
                return Err(message);
            }
            return Ok((
                provider.clone(),
                unconfirmed("登录请求已返回，但响应无法确认；已停止自动重试，请查看站点记录"),
            ));
        }
    };
    let request = apply_session_cookie(
        apply_auth_headers(
            client.get(build_url(&base, "/api/user/self")?),
            &authenticated,
        ),
        &authenticated,
    );
    let result = client
        .send(request, "确认登录签到")
        .await
        .and_then(|response| verify_account(&response, &authenticated));
    let result = if result.is_ok() {
        confirmed()
    } else {
        unconfirmed("重新登录已完成，但账号状态回读失败；请查看站点记录")
    };
    Ok((authenticated, result))
}

pub(crate) async fn check_in_with_browser(
    session: &mut BrowserSession,
    provider: &Provider,
    verification: ProviderCheckInVerification,
    ensure_current: &(dyn Fn() -> Result<(), String> + Send + Sync),
) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, CheckInError> {
    validate(provider)?;
    let base = normalize_base_url(&provider.identity.base_url);
    let public_url = build_url(&base, "/api/status")?;
    let mut needs_token = verification == ProviderCheckInVerification::Turnstile;
    for attempt in 0..2 {
        let mut login_url = build_url(&base, "/api/user/login")?;
        if needs_token {
            let token = turnstile_token(session, public_url.as_str(), ensure_current).await?;
            login_url.query_pairs_mut().append_pair("turnstile", &token);
        }
        ensure_current()?;
        session.phase(CheckInPhase::Requesting);
        let mut response = session.fetch_with_body(login_url.as_str(), "POST", &json!({"content-type":"application/json"}), Some(json!({
            "username": provider.auth.login_username.trim(), "password": provider.auth.login_password,
        }).to_string())).await.map_err(|_| CheckInError::Unconfirmed("登录提交后连接中断，已停止自动重试，请查看站点记录".to_string()))?;
        if let Some(kind) =
            verification_required(response.status, &response.headers, &response.body)
        {
            if attempt == 1 {
                return Err("站点仍拒绝登录验证，请稍后重试".into());
            }
            needs_token = kind == ProviderCheckInVerification::Turnstile;
            session
                .request("navigate", json!({ "path": public_url.as_str() }))
                .await?;
            continue;
        }
        let payload: Value = serde_json::from_str(&response.body).map_err(|_| {
            CheckInError::Unconfirmed("登录响应无法确认，请查看站点记录".to_string())
        })?;
        if response.status != StatusCode::OK
            || payload.get("success").and_then(Value::as_bool) != Some(true)
        {
            return Err(payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("登录被站点拒绝")
                .to_string()
                .into());
        }
        session.phase(CheckInPhase::VerifyingResult);
        let cookies = session.request("cookies", json!({})).await.map_err(|_| {
            CheckInError::Unconfirmed("登录已完成，但无法读取会话；请查看站点记录".to_string())
        })?;
        if let Some(cookies) = cookies.get("cookies").and_then(Value::as_array) {
            for cookie in cookies {
                if cookie.get("name").and_then(Value::as_str) == Some("session") {
                    if let Some(value) = cookie.get("value").and_then(Value::as_str) {
                        response.headers.append(
                            SET_COOKIE,
                            format!("session={value}").parse().map_err(|_| {
                                CheckInError::Unconfirmed("登录已完成，但会话格式无效".to_string())
                            })?,
                        );
                    }
                }
            }
        }
        let authenticated = authenticated_from_login_response(provider, &response)
            .map_err(CheckInError::Unconfirmed)?;
        ensure_current().map_err(CheckInError::Unconfirmed)?;
        let headers = serde_json::to_value(
            auth_header_values(&authenticated)
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .map_err(|_| "无法构造账号确认请求")?;
        let account = session
            .fetch(
                build_url(&base, "/api/user/self")?.as_str(),
                "GET",
                &headers,
            )
            .await
            .map_err(|_| {
                CheckInError::Unconfirmed(
                    "登录已完成，账号状态回读失败，请查看站点记录".to_string(),
                )
            })?;
        verify_account(&account, &authenticated).map_err(CheckInError::Unconfirmed)?;
        return Ok(ProviderOperationOutcome::authenticated(
            provider,
            authenticated,
            confirmed(),
        ));
    }
    Err("登录签到验证未完成".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;
    #[test]
    fn login_confirmation_requires_the_same_account() {
        let mut provider = Provider::from_input(ProviderInput::default(), "fixture".into());
        provider.auth.api_user = "42".into();
        let mut response = TransportResponse {
            status: StatusCode::OK,
            headers: Default::default(),
            body: r#"{"success":true,"data":{"id":42}}"#.into(),
            url: reqwest::Url::parse("https://example.com/api/user/self").unwrap(),
        };
        assert!(verify_account(&response, &provider).is_ok());
        response.body = r#"{"success":true,"data":{"id":43}}"#.into();
        assert!(verify_account(&response, &provider).is_err());
    }
}
