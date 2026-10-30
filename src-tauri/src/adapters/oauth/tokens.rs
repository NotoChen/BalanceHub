use super::http;
use crate::models::{OAuthCredential, OAuthVendor, ProviderAuth};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Client;
use serde_json::{json, Value};

pub(crate) const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(crate) const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub(crate) const GEMINI_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
// Public installed-application client distributed by the open-source Gemini CLI.
const GEMINI_CLIENT_SECRET: &str = "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";
pub(crate) const GROK_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub(crate) const GROK_SCOPE: &str =
    "openid profile email offline_access grok-cli:access api:access";
pub(crate) const CLAUDE_SCOPE: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
pub(crate) const GEMINI_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";

pub(crate) fn client_id(vendor: OAuthVendor) -> &'static str {
    match vendor {
        OAuthVendor::Codex => CODEX_CLIENT_ID,
        OAuthVendor::Claude => CLAUDE_CLIENT_ID,
        OAuthVendor::Gemini => GEMINI_CLIENT_ID,
        OAuthVendor::Grok => GROK_CLIENT_ID,
    }
}
pub(crate) fn scopes(vendor: OAuthVendor) -> &'static str {
    match vendor {
        OAuthVendor::Codex => "openid email profile offline_access",
        OAuthVendor::Claude => CLAUDE_SCOPE,
        OAuthVendor::Gemini => GEMINI_SCOPE,
        OAuthVendor::Grok => GROK_SCOPE,
    }
}
pub(crate) fn token_url(vendor: OAuthVendor) -> &'static str {
    match vendor {
        OAuthVendor::Codex => "https://auth.openai.com/oauth/token",
        OAuthVendor::Claude => "https://platform.claude.com/v1/oauth/token",
        OAuthVendor::Gemini => "https://oauth2.googleapis.com/token",
        OAuthVendor::Grok => "https://auth.x.ai/oauth/token",
    }
}

pub(crate) fn jwt_claims(token: &str) -> Option<Value> {
    // Claims are display hints, never proof of validity. Requests still authenticate
    // with the upstream; parsing never authorizes a local mutation or arbitrary URL.
    if token.len() > 64 * 1024 {
        return None;
    }
    let part = token.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?).ok()
}

pub(crate) fn enrich_identity(auth: &mut ProviderAuth) {
    let Some(identity) = auth.oauth.as_mut() else {
        return;
    };
    for claims in [
        jwt_claims(&identity.id_token),
        jwt_claims(&auth.access_token),
    ]
    .into_iter()
    .flatten()
    {
        if identity.email.is_empty() {
            identity.email = http::text(&claims, &["email"]);
        }
        if identity.subject.is_empty() {
            identity.subject = http::text(&claims, &["sub"]);
        }
        if let Some(openai) = claims.get("https://api.openai.com/auth") {
            if identity.account_id.is_empty() {
                identity.account_id = http::text(openai, &["chatgpt_account_id"]);
            }
            if identity.subject.is_empty() {
                identity.subject = http::text(openai, &["chatgpt_user_id", "user_id"]);
            }
            if identity.plan.is_empty() {
                identity.plan = http::text(openai, &["chatgpt_plan_type"]);
            }
        }
        if identity.vendor == OAuthVendor::Grok {
            if identity.organization_id.is_empty() {
                identity.organization_id = http::text(&claims, &["team_id"]);
            }
            if identity.plan.is_empty() {
                identity.plan = http::text(&claims, &["subscription_tier"]);
            }
            if let Some(tier) = claims.get("tier") {
                let numeric = tier.as_u64().or_else(|| tier.as_str()?.parse().ok());
                identity.plan = match numeric {
                    Some(0) => "Free".into(),
                    Some(1) => "SuperGrok".into(),
                    Some(2) => "X Basic".into(),
                    Some(3) => "X Premium".into(),
                    Some(4) => "X Premium+".into(),
                    Some(5) => "SuperGrok Heavy".into(),
                    Some(6) => "SuperGrok Lite".into(),
                    Some(7) => "SuperGrok Plus".into(),
                    Some(other) => format!("等级 {other}"),
                    None => tier.as_str().unwrap_or(&identity.plan).to_owned(),
                };
            }
        }
    }
    if auth.access_token_expires_at.is_none() {
        auth.access_token_expires_at = jwt_claims(&auth.access_token)
            .and_then(|claims| claims.get("exp").and_then(Value::as_i64))
            .filter(|value| *value >= 0 && chrono::DateTime::from_timestamp(*value, 0).is_some());
    }
    if identity.client_id.is_empty() {
        identity.client_id = client_id(identity.vendor).into();
    }
}

pub(crate) fn apply_response(
    mut auth: ProviderAuth,
    response: &Value,
) -> Result<ProviderAuth, String> {
    let access = http::text(response, &["access_token"]);
    if access.is_empty() {
        return Err("授权服务未返回访问令牌，请重新授权".into());
    }
    auth.access_token = access;
    auth.mode = crate::models::AuthMode::AccessToken;
    auth.source = crate::models::AuthSource::Oauth;
    let refresh = http::text(response, &["refresh_token"]);
    if !refresh.is_empty() {
        auth.refresh_token = refresh;
    }
    auth.access_token_expires_at = response
        .get("expires_in")
        .and_then(http::number)
        .filter(|v| *v > 0.0 && *v < 366.0 * 86400.0)
        .map(|v| chrono::Utc::now().timestamp() + v as i64);
    let identity = auth.oauth.as_mut().ok_or("缺少 OAuth 账号类型")?;
    let id_token = http::text(response, &["id_token"]);
    if !id_token.is_empty() {
        identity.id_token = id_token;
    }
    let scope = http::text(response, &["scope"]);
    if !scope.is_empty() {
        identity.scopes = scope.split_whitespace().map(str::to_owned).collect();
    } else if identity.scopes.is_empty() {
        identity.scopes = scopes(identity.vendor)
            .split_whitespace()
            .map(str::to_owned)
            .collect();
    }
    if let Some(account) = response.get("account") {
        let email = http::text(account, &["email_address", "email"]);
        let id = http::text(account, &["uuid", "id"]);
        if !email.is_empty() {
            identity.email = email;
        }
        if !id.is_empty() {
            identity.subject = id;
        }
    }
    if let Some(organization) = response.get("organization") {
        let id = http::text(organization, &["uuid", "id"]);
        if !id.is_empty() {
            identity.organization_id = id;
        }
        identity.organization_name = http::text(organization, &["name"]);
    }
    identity.refresh_pending = false;
    identity.refresh_error = None;
    identity.updated_at = Some(chrono::Utc::now().timestamp());
    enrich_identity(&mut auth);
    Ok(auth)
}

/// Google does not always issue an ID token for the CLI scopes. Resolve its
/// stable account identity through userinfo; a profile failure must never discard
/// an already exchanged or rotated token.
pub(crate) async fn load_identity(client: &Client, auth: &mut ProviderAuth) -> Result<(), String> {
    let identity = auth.oauth.as_ref().ok_or("缺少 OAuth 账号类型")?;
    let vendor = identity.vendor;
    let url = match vendor {
        OAuthVendor::Gemini => "https://www.googleapis.com/oauth2/v2/userinfo",
        OAuthVendor::Claude => "https://api.anthropic.com/api/oauth/profile",
        _ => return Ok(()),
    };
    if auth.access_token.is_empty() {
        return Ok(());
    }
    if vendor == OAuthVendor::Gemini && !identity.email.is_empty() && !identity.subject.is_empty() {
        return Ok(());
    }
    let mut request = client
        .get(url)
        .bearer_auth(&auth.access_token)
        .timeout(std::time::Duration::from_secs(10));
    if vendor == OAuthVendor::Claude {
        request = request.header("anthropic-beta", "oauth-2025-04-20");
    }
    let value = http::json(request, "读取账号身份").await?;
    apply_identity(auth, &value);
    Ok(())
}

pub(crate) fn apply_identity(auth: &mut ProviderAuth, value: &Value) {
    let Some(identity) = auth.oauth.as_mut() else {
        return;
    };
    let account = value.get("account").unwrap_or(value);
    for (target, names) in [
        (&mut identity.email, &["email", "email_address"][..]),
        (&mut identity.subject, &["id", "uuid", "sub"][..]),
    ] {
        let text = http::text(account, names);
        if !text.is_empty() {
            *target = text;
        }
    }
    if identity.vendor == OAuthVendor::Claude {
        if let Some(organization) = value.get("organization") {
            for (target, names) in [
                (&mut identity.organization_id, &["uuid", "id"][..]),
                (&mut identity.organization_name, &["name"][..]),
                (
                    &mut identity.plan,
                    &["subscription_type", "organization_type", "billing_type"][..],
                ),
            ] {
                let text = http::text(organization, names);
                if !text.is_empty() {
                    *target = text;
                }
            }
        }
    }
}

pub(crate) async fn exchange(
    client: &Client,
    vendor: OAuthVendor,
    code: &str,
    redirect: &str,
    verifier: &str,
    state: &str,
) -> Result<ProviderAuth, String> {
    let mut auth = crate::models::ProviderInput::default().auth;
    auth.oauth = Some(OAuthCredential::new(vendor));
    let mut payload = json!({"grant_type":"authorization_code", "client_id":client_id(vendor), "code":code, "redirect_uri":redirect, "code_verifier":verifier});
    if vendor == OAuthVendor::Claude {
        payload["state"] = state.into();
    }
    if vendor == OAuthVendor::Gemini {
        payload["client_secret"] = GEMINI_CLIENT_SECRET.into();
    }
    let request = client.post(token_url(vendor));
    let request = if vendor == OAuthVendor::Claude {
        request.json(&payload)
    } else {
        request.form(&payload)
    };
    let response = http::json(request, "交换授权").await?;
    let mut auth = apply_response(auth, &response)?;
    let identity = auth.oauth.as_mut().expect("OAuth identity initialized");
    if identity.scopes.is_empty() {
        identity.scopes = scopes(vendor)
            .split_whitespace()
            .map(str::to_owned)
            .collect();
    }
    let _ = load_identity(client, &mut auth).await;
    Ok(auth)
}

pub(crate) async fn refresh(client: &Client, auth: &ProviderAuth) -> Result<ProviderAuth, String> {
    let identity = auth.oauth.as_ref().ok_or("缺少 OAuth 账号")?;
    if auth.refresh_token.trim().is_empty() {
        return Err("此授权没有刷新令牌，请重新授权".into());
    }
    let id = if identity.client_id.is_empty() {
        client_id(identity.vendor)
    } else {
        &identity.client_id
    };
    let mut payload =
        json!({"grant_type":"refresh_token", "refresh_token":auth.refresh_token, "client_id":id});
    if identity.vendor == OAuthVendor::Gemini {
        let secret = if identity.client_secret.is_empty() && id == GEMINI_CLIENT_ID {
            GEMINI_CLIENT_SECRET
        } else {
            &identity.client_secret
        };
        if secret.is_empty() {
            return Err("此 Google 授权缺少原 OAuth 客户端信息，请重新授权".into());
        }
        payload["client_secret"] = secret.into();
    }
    if matches!(identity.vendor, OAuthVendor::Claude | OAuthVendor::Grok) {
        payload["scope"] = scopes(identity.vendor).into();
    }
    let token_endpoint = if identity.vendor == OAuthVendor::Grok {
        discover_grok(client).await?.1
    } else {
        token_url(identity.vendor).to_string()
    };
    let request = client.post(token_endpoint);
    let request = if identity.vendor == OAuthVendor::Claude {
        request.json(&payload)
    } else {
        request.form(&payload)
    };
    apply_response(auth.clone(), &http::json(request, "续期授权").await?)
}

pub(crate) async fn discover_grok(client: &Client) -> Result<(String, String), String> {
    let data = http::json(
        client.get("https://auth.x.ai/.well-known/openid-configuration"),
        "读取 Grok 授权入口",
    )
    .await?;
    let endpoint = |field: &str| -> Result<String, String> {
        let raw = data
            .get(field)
            .and_then(Value::as_str)
            .ok_or("Grok 授权服务未提供所需入口")?;
        let url = reqwest::Url::parse(raw).map_err(|_| "Grok 授权入口无效")?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || !url
                .host_str()
                .is_some_and(|h| h == "x.ai" || h.ends_with(".x.ai"))
        {
            return Err("Grok 授权入口不属于官方域名".into());
        }
        Ok(raw.to_string())
    };
    Ok((
        endpoint("device_authorization_endpoint")?,
        endpoint("token_endpoint")?,
    ))
}
