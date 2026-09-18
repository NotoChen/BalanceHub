//! Fresh login confirms a session/account, not the site's reward credit.
use super::{
    challenge::{verification_required, verification_result},
    executor::{Credentials, Executor},
};
use crate::{
    adapters::{
        new_api::{
            http::{authenticated_from_login_response, build_url},
            response::parse_success_data,
        },
        transport::TransportResponse,
    },
    models::{
        CheckInError, CheckInPhase, Provider, ProviderCheckInResult, ProviderCheckInVerification,
    },
};
use reqwest::Method;
use serde_json::{json, Value};

pub(super) async fn run(
    executor: &mut Executor<'_>,
    provider: &mut Provider,
    needs_token: bool,
) -> Result<ProviderCheckInResult, CheckInError> {
    if let Some(result) = login(executor, provider, needs_token).await? {
        return Ok(result);
    }
    executor.phase(CheckInPhase::VerifyingResult);
    let account_url = build_url(&provider.identity.base_url, "/api/user/self")?;
    let response = executor
        .read(provider, &account_url, true)
        .await
        .map_err(|_| {
            CheckInError::Unconfirmed("重新登录已完成，但账号状态回读失败；请查看站点记录".into())
        })?;
    verify_account(&response, provider).map_err(CheckInError::Unconfirmed)?;
    Ok(ProviderCheckInResult {
        ok: true,
        message: "已重新登录并确认账号状态；奖励以站点记录为准".into(),
        unconfirmed: false,
        verification_required: None,
        verification_requires_login: false,
        last_checked_in_at: None,
        last_check_in_user: None,
        quota_delta: None,
    })
}

/// Shared with password authentication before standard/session sign-in.
/// Some(result) yields to verification; None means the new credentials are ready.
pub(super) async fn login(
    executor: &mut Executor<'_>,
    provider: &mut Provider,
    mut needs_token: bool,
) -> Result<Option<ProviderCheckInResult>, CheckInError> {
    let public_url = build_url(&provider.identity.base_url, "/api/status")?;
    for attempt in 0..2 {
        let mut login_url = build_url(&provider.identity.base_url, "/api/user/login")?;
        if needs_token {
            let Some(token) = executor.turnstile_token(provider).await? else {
                return Ok(Some(login_verification_result(
                    ProviderCheckInVerification::Turnstile,
                )));
            };
            login_url.query_pairs_mut().append_pair("turnstile", &token);
        }
        executor.phase(CheckInPhase::Requesting);
        let mut response = executor.send(provider, &login_url, Method::POST, Credentials::None, Some(json!({
            "username": provider.auth.login_username.trim(), "password": provider.auth.login_password,
        }).to_string())).await.map_err(|_| CheckInError::Unconfirmed(
            "登录提交后连接中断，结果尚未确认；已停止自动重试，请查看站点记录".into(),
        ))?;
        if let Some(kind) =
            verification_required(response.status, &response.headers, &response.body)
        {
            if !executor
                .retry_verification(provider, kind, &public_url)
                .await?
            {
                return Ok(Some(login_verification_result(kind)));
            }
            if attempt == 1 {
                return Err("站点仍拒绝登录验证，已停止重试".into());
            }
            needs_token |= kind == ProviderCheckInVerification::Turnstile;
            continue;
        }
        let payload: Value = serde_json::from_str(&response.body).map_err(|_| {
            CheckInError::Unconfirmed(
                "登录请求已返回，但响应无法确认；已停止自动重试，请查看站点记录".into(),
            )
        })?;
        if payload.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("登录被站点拒绝")
                .to_string()
                .into());
        }
        executor
            .collect_login_cookie(&mut response)
            .await
            .map_err(|_| {
                CheckInError::Unconfirmed("登录请求已完成，但无法读取会话；请查看站点记录".into())
            })?;
        let authenticated = authenticated_from_login_response(provider, &response)
            .map_err(CheckInError::Unconfirmed)?;
        if !provider.auth.api_user.trim().is_empty()
            && provider.auth.api_user != authenticated.auth.api_user
        {
            return Err(CheckInError::Unconfirmed(
                "重新登录的账号与当前配置不一致，已停止写入会话".into(),
            ));
        }
        *provider = authenticated;
        return Ok(None);
    }
    Err("登录验证未完成".into())
}

fn login_verification_result(kind: ProviderCheckInVerification) -> ProviderCheckInResult {
    let mut result = verification_result(kind);
    result.verification_requires_login = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;
    use reqwest::StatusCode;

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
