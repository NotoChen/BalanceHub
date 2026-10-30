use crate::{
    adapters::oauth::{http, tokens},
    models::*,
};
use serde_json::Value;

mod export;
pub(crate) use export::{export, options, source};

pub(crate) fn same_account(a: &ProviderAuth, b: &ProviderAuth) -> bool {
    let (Some(aid), Some(bid)) = (&a.oauth, &b.oauth) else {
        return false;
    };
    if aid.vendor != bid.vendor {
        return false;
    }
    for (left, right) in [
        (&aid.account_id, &bid.account_id),
        (&aid.organization_id, &bid.organization_id),
        (&aid.project_id, &bid.project_id),
        (&aid.subject, &bid.subject),
    ] {
        if !left.is_empty() && !right.is_empty() && left != right {
            return false;
        }
    }
    if !a.refresh_token.is_empty() && a.refresh_token == b.refresh_token {
        return true;
    }
    if !a.access_token.is_empty() && a.access_token == b.access_token {
        return true;
    }
    if aid.account_id != bid.account_id
        || aid.organization_id != bid.organization_id
        || aid.project_id != bid.project_id
    {
        return false;
    }
    if !aid.subject.is_empty() && !bid.subject.is_empty() {
        return aid.subject == bid.subject;
    }
    !aid.email.is_empty() && !bid.email.is_empty() && aid.email.eq_ignore_ascii_case(&bid.email)
}

fn vendor(value: &str) -> Option<OAuthVendor> {
    match value.to_ascii_lowercase().as_str() {
        "codex" | "openai" | "chatgpt" => Some(OAuthVendor::Codex),
        "claude" | "anthropic" => Some(OAuthVendor::Claude),
        "gemini" | "gemini-cli" => Some(OAuthVendor::Gemini),
        "grok" | "xai" => Some(OAuthVendor::Grok),
        _ => None,
    }
}

pub(crate) fn parse_file(
    file: &OAuthImportFile,
    hint: Option<OAuthVendor>,
) -> Result<Vec<Result<ProviderAuth, String>>, String> {
    if file.content.len() > 4 * 1024 * 1024 {
        return Err("授权文件超过 4 MiB，请只选择授权文件".into());
    }
    let value: Value = serde_json::from_str(file.content.trim_start_matches('\u{feff}'))
        .map_err(|_| "授权文件不是有效 JSON")?;
    if value.get("format").and_then(Value::as_str) == Some("balancehub-oauth")
        && value.get("version").and_then(Value::as_u64) != Some(1)
    {
        return Err("此 BalanceHub 授权文件版本不受支持".into());
    }
    let filename = file.name.to_lowercase();
    let hint = if filename == "codex_oauth_auth.json" {
        Some(OAuthVendor::Codex)
    } else if filename == "xai_oauth_auth.json" {
        Some(OAuthVendor::Grok)
    } else {
        hint
    };
    let mut result = Vec::new();
    walk(&value, hint, 0, &mut result)?;
    Ok(result)
}

fn walk(
    value: &Value,
    hint: Option<OAuthVendor>,
    depth: usize,
    out: &mut Vec<Result<ProviderAuth, String>>,
) -> Result<(), String> {
    if depth > 8 || out.len() >= 256 {
        return Err("授权文件结构过深或账号过多，请分批导入".into());
    }
    if let Some(values) = value.as_array() {
        for value in values {
            walk(value, hint, depth + 1, out)?;
        }
        return Ok(());
    }
    let Some(object) = value.as_object() else {
        out.push(Err("条目不是授权对象".into()));
        return Ok(());
    };
    if let Some(accounts) = object.get("accounts") {
        if let Some(accounts) = accounts.as_object() {
            for account in accounts.values() {
                walk(account, hint, depth + 1, out)?;
            }
        } else {
            walk(accounts, hint, depth + 1, out)?;
        }
        return Ok(());
    }
    if let Some(data) = object
        .get("data")
        .filter(|v| v.get("accounts").is_some() || v.is_array())
    {
        return walk(data, hint, depth + 1, out);
    }
    if let Some(channels) = object.get("channels").filter(|v| v.is_array()) {
        return walk(channels, hint, depth + 1, out);
    }
    if let Some(key) = object
        .get("key")
        .and_then(Value::as_str)
        .filter(|v| v.trim_start().starts_with('{'))
    {
        let nested: Value = serde_json::from_str(key).map_err(|_| "渠道中的授权 JSON 无效")?;
        return walk(&nested, hint, depth + 1, out);
    }
    // Grok CLI's native store uses issuer/client keys. These are OAuth access tokens,
    // not the unrelated legacy grok.com browser SSO cookies.
    let native: Vec<_> = object
        .iter()
        .filter(|(key, _)| key.starts_with("https://auth.x.ai::"))
        .collect();
    if !native.is_empty() {
        for (scope, credential) in native {
            let mut auth = parse_entry(credential, Some(OAuthVendor::Grok));
            if let Ok(auth) = &mut auth {
                auth.oauth.as_mut().expect("parsed").client_id =
                    scope.trim_start_matches("https://auth.x.ai::").into();
            }
            out.push(auth);
        }
        return Ok(());
    }
    out.push(parse_entry(value, hint));
    Ok(())
}

fn parse_entry(value: &Value, hint: Option<OAuthVendor>) -> Result<ProviderAuth, String> {
    if value.get("format").and_then(Value::as_str) == Some("balancehub-oauth")
        && value.get("version").and_then(Value::as_u64) != Some(1)
    {
        return Err("此 BalanceHub 授权文件版本不受支持".into());
    }
    let explicit = http::text(value, &["platform", "vendor", "type"]);
    if ["platform", "vendor", "type", "auth_mode"]
        .iter()
        .any(|key| {
            value.get(key).and_then(Value::as_str).is_some_and(|kind| {
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "antigravity" | "sso" | "cookie" | "siwc" | "apikey" | "api_key"
                )
            })
        })
    {
        return Err("此授权不是 Gemini CLI 或 Grok OAuth，不能转换为当前账号类型".into());
    }
    let mut kind = vendor(&explicit).or(hint);
    let source = if value.get("claudeAiOauth").is_some() {
        kind = Some(OAuthVendor::Claude);
        "Claude Code"
    } else if value.get("tokens").is_some() {
        kind = Some(OAuthVendor::Codex);
        "Codex CLI"
    } else if value.get("credentials").is_some() {
        "Sub2API"
    } else if value.get("chatgpt_account_id").is_some() || value.get("authenticated_at").is_some() {
        if value.get("chatgpt_account_id").is_some() {
            kind = Some(OAuthVendor::Codex);
        } else if value.get("requires_reauth").is_some() && value.get("login").is_some() {
            kind = Some(OAuthVendor::Grok);
        }
        "CC Switch"
    } else if value.get("expiry_date").is_some() {
        kind = Some(OAuthVendor::Gemini);
        "Gemini CLI"
    } else if explicit == "xai" || explicit == "codex" || explicit == "claude" {
        "CPA / NewAPI / Magpie"
    } else {
        "授权文件"
    };
    let nested = value
        .get("credentials")
        .or_else(|| value.get("tokens"))
        .or_else(|| value.get("claudeAiOauth"))
        .or_else(|| value.get("token").filter(|v| v.is_object()))
        .unwrap_or(value);
    let raw_access = http::text(nested, &["access_token", "accessToken", "access"]);
    let id_token = http::text(nested, &["id_token", "idToken"]);
    if kind.is_none() {
        if let Some(claims) =
            tokens::jwt_claims(&id_token).or_else(|| tokens::jwt_claims(&raw_access))
        {
            if claims.get("https://api.openai.com/auth").is_some() {
                kind = Some(OAuthVendor::Codex);
            } else if claims.get("iss").and_then(Value::as_str) == Some("https://auth.x.ai") {
                kind = Some(OAuthVendor::Grok);
            }
        }
    }
    let Some(kind) = kind else {
        return Err("无法确定授权所属服务，请选择 Codex、Claude、Gemini 或 Grok 后重新读取".into());
    };
    if let Some(expected) = hint {
        if kind != expected {
            return Err("授权文件的服务与所选服务不一致".into());
        }
    }
    if kind == OAuthVendor::Gemini
        && !matches!(
            http::text(nested, &["oauth_type"]).as_str(),
            "" | "code_assist"
        )
    {
        return Err("此文件是 AI Studio OAuth，与 Gemini CLI 的 Code Assist 授权不同".into());
    }
    let mut auth = ProviderInput::default().auth;
    auth.mode = AuthMode::AccessToken;
    auth.source = AuthSource::Oauth;
    auth.access_token = raw_access;
    if auth.access_token.is_empty() && kind == OAuthVendor::Grok {
        auth.access_token = http::text(nested, &["key"]);
    }
    auth.refresh_token = http::text(nested, &["refresh_token", "refreshToken", "refresh"]);
    if auth.access_token.is_empty() && auth.refresh_token.is_empty() {
        return Err("条目没有 OAuth 访问令牌或刷新令牌；API Key 不属于账号授权".into());
    }
    if [&auth.access_token, &auth.refresh_token].iter().any(|v| {
        v.contains('*')
            || v.contains('•')
            || v.len() > 128 * 1024
            || v.chars().any(char::is_whitespace)
    }) {
        return Err("授权包含脱敏或无效令牌，请导入原始授权文件".into());
    }
    let mut identity = OAuthCredential::new(kind);
    identity.source = source.into();
    identity.id_token = id_token;
    identity.email = http::text(nested, &["email", "email_address", "login"]);
    if identity.email.is_empty() {
        identity.email = http::text(value, &["email", "login"]);
    }
    identity.account_id = if source == "CC Switch" && kind == OAuthVendor::Codex {
        http::text(nested, &["chatgpt_account_id"])
    } else {
        http::text(nested, &["chatgpt_account_id", "account_id", "accountId"])
    };
    identity.subject = http::text(
        nested,
        &[
            "sub",
            "subject",
            "account_uuid",
            "chatgpt_user_id",
            "claude_ai_user_id",
            "user_id",
        ],
    );
    if source == "CC Switch" && kind == OAuthVendor::Grok {
        identity.subject = http::text(nested, &["account_id"]);
        identity.account_id.clear();
    }
    identity.organization_id = http::text(
        nested,
        &[
            "organization_uuid",
            "organization_id",
            "org_uuid",
            "team_id",
        ],
    );
    identity.organization_name = http::text(nested, &["organization_name"]);
    if let Some(extra) = value
        .get("extra")
        .filter(|_| value.get("credentials").is_some())
    {
        // Sub2API can keep Claude's identity in extra while credentials contains
        // only tokens. Explicit credential fields remain authoritative.
        for (target, names) in [
            (&mut identity.email, &["email_address", "email"][..]),
            (&mut identity.subject, &["account_uuid"][..]),
            (&mut identity.organization_id, &["org_uuid"][..]),
        ] {
            if target.is_empty() {
                *target = http::text(extra, names);
            }
        }
    }
    identity.project_id = http::text(nested, &["project_id", "projectId"]);
    if identity.project_id.is_empty() {
        identity.project_id = http::text(value, &["project_id"]);
    }
    identity.plan = http::text(
        nested,
        &[
            "plan_type",
            "subscriptionType",
            "plan",
            "tier_id",
            "subscription_tier",
        ],
    );
    if identity.plan.is_empty() {
        identity.plan = http::text(value, &["plan_type", "plan"]);
    }
    identity.client_id = http::text(nested, &["client_id", "clientId"]);
    identity.client_secret = http::text(nested, &["client_secret", "clientSecret"]);
    identity.scopes = match nested.get("scopes").or_else(|| nested.get("scope")) {
        Some(Value::Array(v)) => v
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(v)) => v.split_whitespace().map(str::to_owned).collect(),
        _ => Vec::new(),
    };
    identity.updated_at = nested
        .get("last_refresh")
        .or_else(|| nested.get("token_updated_at_ms"))
        .and_then(http::timestamp);
    auth.access_token_expires_at = [
        "expired",
        "expires_at",
        "expiresAt",
        "expiry_date",
        "expires",
    ]
    .iter()
    .find_map(|key| nested.get(key).and_then(http::timestamp));
    auth.oauth = Some(identity);
    tokens::enrich_identity(&mut auth);
    Ok(auth)
}
