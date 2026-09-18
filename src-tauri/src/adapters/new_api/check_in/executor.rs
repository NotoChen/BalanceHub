use super::{
    super::http::{
        apply_auth_headers, apply_session_cookie, auth_header_values, build_url,
        normalize_base_url, USER_AGENT_VALUE,
    },
    challenge::verification_required,
};
use crate::{
    adapters::{
        browser::BrowserSession,
        transport::{ProviderTransport, TransportResponse},
    },
    models::{CheckInError, CheckInPhase, Provider, ProviderCheckInVerification},
};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT},
    Method, Url,
};
use serde_json::{json, Value};

pub(super) enum Credentials {
    Account,
    SessionOnly,
    None,
}

/// Only transport and verification differ. Business requests live in the method.
pub(super) enum Executor<'a> {
    Http(&'a ProviderTransport),
    Browser {
        session: &'a mut BrowserSession,
        ensure_current: &'a (dyn Fn() -> Result<(), String> + Send + Sync),
    },
}

impl Executor<'_> {
    pub(super) fn is_browser(&self) -> bool {
        matches!(self, Self::Browser { .. })
    }

    pub(super) fn phase(&self, phase: CheckInPhase) {
        if let Self::Browser { session, .. } = self {
            session.phase(phase);
        }
    }

    pub(super) async fn send(
        &mut self,
        provider: &Provider,
        url: &Url,
        method: Method,
        credentials: Credentials,
        body: Option<String>,
    ) -> Result<TransportResponse, CheckInError> {
        match self {
            Self::Http(client) => {
                let base = normalize_base_url(&provider.identity.base_url);
                let mut request = client
                    .request(method, url.clone())
                    .header(USER_AGENT, USER_AGENT_VALUE)
                    .header(CONTENT_TYPE, "application/json")
                    .header(ACCEPT, "application/json, text/plain, */*")
                    .header(ORIGIN, &base)
                    .header(REFERER, format!("{base}/"))
                    .header("X-Requested-With", "XMLHttpRequest");
                request = match credentials {
                    Credentials::Account => {
                        apply_session_cookie(apply_auth_headers(request, provider), provider)
                    }
                    Credentials::SessionOnly => request.header(
                        COOKIE,
                        format!(
                            "session={}",
                            super::session_sign_in::normalize_session_cookie(
                                &provider.auth.session_cookie
                            )
                        ),
                    ),
                    Credentials::None => request,
                };
                if let Some(body) = body {
                    request = request.body(body);
                }
                client.send(request, "签到请求").await.map_err(Into::into)
            }
            Self::Browser {
                session,
                ensure_current,
            } => {
                ensure_current()?;
                if matches!(credentials, Credentials::None) && method == Method::POST {
                    session.request("clearSession", json!({})).await?;
                }
                let mut headers = serde_json::Map::new();
                if matches!(credentials, Credentials::Account) {
                    for (name, value) in auth_header_values(provider) {
                        headers.insert(name.to_string(), Value::String(value));
                    }
                }
                headers.insert("content-type".into(), json!("application/json"));
                headers.insert("accept".into(), json!("application/json"));
                headers.insert("x-requested-with".into(), json!("XMLHttpRequest"));
                session
                    .fetch_with_body(url.as_str(), method.as_str(), &Value::Object(headers), body)
                    .await
            }
        }
    }

    pub(super) async fn read(
        &mut self,
        provider: &Provider,
        url: &Url,
        authenticated: bool,
    ) -> Result<TransportResponse, CheckInError> {
        let credentials = || {
            if authenticated {
                Credentials::Account
            } else {
                Credentials::None
            }
        };
        let response = self
            .send(provider, url, Method::GET, credentials(), None)
            .await?;
        if self.is_browser()
            && verification_required(response.status, &response.headers, &response.body)
                == Some(ProviderCheckInVerification::Cloudflare)
        {
            self.retry_verification(provider, ProviderCheckInVerification::Cloudflare, url)
                .await?;
            return self
                .send(provider, url, Method::GET, credentials(), None)
                .await;
        }
        Ok(response)
    }

    /// False hands an explicitly rejected operation back to the browser queue.
    pub(super) async fn retry_verification(
        &mut self,
        provider: &Provider,
        kind: ProviderCheckInVerification,
        safe_page: &Url,
    ) -> Result<bool, CheckInError> {
        if kind != ProviderCheckInVerification::Turnstile && !provider.automation.auto_shield {
            return Err("站点需要页面验证；当前已关闭自动处理站点防护".into());
        }
        match self {
            Self::Http(_) => Ok(false),
            Self::Browser {
                session,
                ensure_current,
            } => {
                ensure_current()?;
                if kind != ProviderCheckInVerification::Turnstile {
                    session
                        .request("navigate", json!({ "path": safe_page.as_str() }))
                        .await?;
                }
                Ok(true)
            }
        }
    }

    /// Tokens are acquired and consumed in the same browser, never cached.
    pub(super) async fn turnstile_token(
        &mut self,
        provider: &Provider,
    ) -> Result<Option<String>, CheckInError> {
        if !self.is_browser() {
            return Ok(None);
        }
        let public_url = build_url(&provider.identity.base_url, "/api/status")?;
        let metadata = self.read(provider, &public_url, false).await?;
        if !metadata.status.is_success() {
            return Err(format!("读取验证配置失败：HTTP {}", metadata.status.as_u16()).into());
        }
        let metadata: Value =
            serde_json::from_str(&metadata.body).map_err(|_| "站点没有返回有效的验证配置")?;
        let site_key = metadata
            .pointer("/data/turnstile_site_key")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
            .ok_or("站点要求 Turnstile，但没有提供验证配置")?;
        let Self::Browser {
            session,
            ensure_current,
        } = self
        else {
            unreachable!()
        };
        ensure_current()?;
        let verification = session
            .request("verify", json!({ "siteKey": site_key }))
            .await?;
        verification
            .get("token")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(|token| Some(token.to_string()))
            .ok_or_else(|| "未取得签到验证结果".into())
    }

    pub(super) async fn collect_login_cookie(
        &mut self,
        response: &mut TransportResponse,
    ) -> Result<(), CheckInError> {
        if let Self::Browser {
            session,
            ensure_current,
        } = self
        {
            ensure_current()?;
            let refresh_url = response
                .url
                .as_str()
                .split("/api/user/login")
                .next()
                .unwrap_or(response.url.as_str())
                .to_string()
                + "/api/user/auth/refresh";
            let cookies = session
                .request("cookies", json!({ "url": refresh_url }))
                .await?;
            if let Some(cookies) = cookies.get("cookies").and_then(Value::as_array) {
                for cookie in cookies {
                    if let Some(name @ ("session" | "new_api_refresh")) =
                        cookie.get("name").and_then(Value::as_str)
                    {
                        if let Some(value) = cookie.get("value").and_then(Value::as_str) {
                            response.headers.append(
                                SET_COOKIE,
                                format!("{name}={value}")
                                    .parse()
                                    .map_err(|_| "登录返回的会话格式无效")?,
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
