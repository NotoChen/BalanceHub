//! NewAPI v1 dashboard JWTs and rotating refresh cookies. PATs stay separate.
use crate::{
    adapters::{protocol::contracts::ProviderOperationOutcome, transport::TransportResponse},
    models::{AppSettings, AuthMode, NewApiSession, Provider},
    util::unix_secs,
};
use reqwest::header::{ACCEPT, COOKIE, ORIGIN, REFERER, SET_COOKIE};
use serde_json::Value;

use super::http::{build_client, build_url, normalize_base_url, ProviderTransport};

pub(super) enum SessionError {
    Expired,
    Failed(String),
}

impl SessionError {
    pub(super) fn message(&self) -> String {
        match self {
            Self::Expired => "登录会话已过期或已撤销，请重新登录并导入".to_string(),
            Self::Failed(message) => message.clone(),
        }
    }
}

pub(super) fn cookie_value(raw: &str, name: &str) -> Option<String> {
    raw.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim() == name && !value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

fn response_refresh_cookie(response: &TransportResponse) -> Option<String> {
    response
        .headers
        .get_all(SET_COOKIE)
        .iter()
        .find_map(|header| {
            cookie_value(header.to_str().ok()?.split(';').next()?, "new_api_refresh")
        })
}

pub(super) fn active(provider: &Provider) -> bool {
    matches!(provider.auth.mode, AuthMode::Session | AuthMode::Password)
        && (provider.auth.new_api_session.is_some()
            || cookie_value(&provider.auth.session_cookie, "new_api_refresh").is_some())
}

pub(super) fn needs_refresh(session: &NewApiSession, now: i64) -> bool {
    session.access_token.is_empty()
        || session
            .access_expires_at
            .is_none_or(|expiry| expiry <= now + 60)
}

pub(super) fn authenticated_bundle(
    provider: &Provider,
    response: &TransportResponse,
    data: &Value,
) -> Result<Provider, String> {
    let token = data
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("登录响应缺少访问令牌")?;
    let user = data.get("user").ok_or("登录响应缺少账号信息")?;
    let user_id = user
        .get("id")
        .and_then(|value| {
            value
                .as_i64()
                .map(|v| v.to_string())
                .or_else(|| value.as_str().map(str::to_string))
        })
        .filter(|s| !s.is_empty())
        .ok_or("登录响应缺少用户 ID")?;
    if !provider.auth.api_user.is_empty() && provider.auth.api_user != user_id {
        return Err("登录响应的账号与当前中转站不一致，请重新导入".to_string());
    }
    let session_id = data
        .pointer("/session/sid")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("登录响应缺少会话标识")?;
    let refresh_cookie = response_refresh_cookie(response).ok_or("登录响应缺少刷新 Cookie")?;
    let access_expires_at = data
        .get("access_expires_at")
        .and_then(Value::as_i64)
        .filter(|expiry| *expiry > 0)
        .ok_or("登录响应缺少令牌有效期，无法自动维护会话")?;
    let mut authenticated = provider.clone();
    authenticated.auth.mode = AuthMode::Session;
    authenticated.auth.api_user = user_id;
    // Business APIs only receive the JWT; never forward refresh credentials to
    // quota/check-in endpoints or mix a new session with an old classic cookie.
    authenticated.auth.session_cookie.clear();
    authenticated.auth.new_api_session = Some(NewApiSession {
        refresh_cookie,
        session_id: session_id.to_string(),
        access_token: token.to_string(),
        access_expires_at: Some(access_expires_at),
    });
    if authenticated.auth.login_username.is_empty() {
        authenticated.auth.login_username = user
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    }
    Ok(authenticated)
}

pub(super) async fn authenticate(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Provider, SessionError> {
    if let Some(session) = &provider.auth.new_api_session {
        if !needs_refresh(session, unix_secs() as i64) {
            let mut authenticated = provider.clone();
            authenticated.auth.mode = AuthMode::Session;
            return Ok(authenticated);
        }
    }
    let refresh_cookie = provider
        .auth
        .new_api_session
        .as_ref()
        .map(|s| s.refresh_cookie.clone())
        .or_else(|| cookie_value(&provider.auth.session_cookie, "new_api_refresh"))
        .filter(|s| !s.is_empty())
        .ok_or(SessionError::Expired)?;
    let base = normalize_base_url(&provider.identity.base_url);
    let url = build_url(&base, "/api/user/auth/refresh").map_err(SessionError::Failed)?;
    let origin = url.origin().ascii_serialization();
    let request = client
        .post(url)
        .header(COOKIE, format!("new_api_refresh={refresh_cookie}"))
        .header(ORIGIN, origin)
        .header(REFERER, format!("{base}/"))
        .header(ACCEPT, "application/json");
    let response = client
        .send(request, "续期登录会话")
        .await
        .map_err(SessionError::Failed)?;
    let payload: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    if response.status.as_u16() == 401 {
        return Err(SessionError::Expired);
    }
    if !response.status.is_success()
        || payload.get("success").and_then(Value::as_bool) != Some(true)
    {
        return Err(SessionError::Failed(format!(
            "登录会话续期失败（HTTP {}），可稍后重试",
            response.status.as_u16()
        )));
    }
    authenticated_bundle(provider, &response, &payload["data"]).map_err(SessionError::Failed)
}

/// Service preflight persists the new refresh cookie before the business API.
/// Expiry invalidation is also an explicit patch even though the operation fails.
pub(crate) async fn prepare_authentication(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<ProviderOperationOutcome<Result<(), String>>, String> {
    let client = build_client(settings, provider).await?;
    let authenticated = if active(provider) {
        match authenticate(&client, provider).await {
            Ok(value) => value,
            Err(SessionError::Expired) => {
                let mut expired = provider.clone();
                expired.auth.new_api_session = None;
                expired.auth.session_cookie.clear();
                return Ok(ProviderOperationOutcome::authenticated(
                    provider,
                    expired,
                    Err(SessionError::Expired.message()),
                ));
            }
            Err(error) => return Err(error.message()),
        }
    } else {
        super::http::authenticate_password_provider(&client, provider).await?
    };
    Ok(ProviderOperationOutcome::authenticated(
        provider,
        authenticated,
        Ok(()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;
    use reqwest::{header::HeaderMap, StatusCode, Url};

    #[test]
    fn modern_login_keeps_pat_separate_and_captures_httponly_refresh_cookie() {
        let mut original = Provider::from_input(ProviderInput::default(), "fixture".into());
        original.auth.access_token = "configured-pat".into();
        let mut headers = HeaderMap::new();
        headers.insert(
            SET_COOKIE,
            "new_api_refresh=rotated; Path=/api/user/auth; HttpOnly; Secure"
                .parse()
                .unwrap(),
        );
        let response = TransportResponse {
            status: StatusCode::OK,
            headers,
            body: String::new(),
            url: Url::parse("https://example.test/api/user/login").unwrap(),
        };
        let data = serde_json::json!({ "access_token": "dashboard-jwt", "access_expires_at": 900, "user": {"id": 42}, "session": {"sid": "login-session"} });
        let authenticated = authenticated_bundle(&original, &response, &data).unwrap();
        assert_eq!(authenticated.auth.access_token, "configured-pat");
        let session = authenticated.auth.new_api_session.unwrap();
        assert_eq!(session.refresh_cookie, "rotated");
        assert!(!needs_refresh(&session, 839));
        assert!(needs_refresh(&session, 840));
        assert!(authenticated.auth.session_cookie.is_empty());
        let mut missing_expiry = data;
        missing_expiry
            .as_object_mut()
            .unwrap()
            .remove("access_expires_at");
        assert!(authenticated_bundle(&original, &response, &missing_expiry).is_err());
    }

    #[test]
    fn explicit_pat_never_consumes_a_cached_dashboard_refresh_cookie() {
        let mut provider = Provider::from_input(ProviderInput::default(), "fixture".into());
        provider.auth.mode = AuthMode::AccessToken;
        provider.auth.session_cookie = "new_api_refresh=unused".into();
        assert!(!active(&provider));
    }
}
