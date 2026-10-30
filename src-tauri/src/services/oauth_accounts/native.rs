//! The authorization documents the vendor CLIs read from their own
//! configuration directory.
//!
//! Both handoff paths — the isolated temporary CLI launch and switching the
//! agent's native authorization — write this document, so the accepted shape
//! only has one definition. It mirrors `OAuthExportFormat::Native`.

use crate::models::{OAuthCredential, OAuthVendor, ProviderAuth};
use serde_json::{json, Value};

/// File name inside the agent's own configuration directory.
pub(crate) const fn file_name(vendor: OAuthVendor) -> &'static str {
    match vendor {
        OAuthVendor::Codex | OAuthVendor::Grok => "auth.json",
        OAuthVendor::Claude => ".credentials.json",
        OAuthVendor::Gemini => "oauth_creds.json",
    }
}

/// Builds the document the CLI expects. `include_refresh` is false for the
/// temporary launch: a throwaway directory must not be able to rotate the
/// refresh token that BalanceHub still owns.
pub(crate) fn document(
    vendor: OAuthVendor,
    auth: &ProviderAuth,
    identity: &OAuthCredential,
    include_refresh: bool,
) -> Value {
    let refresh = if include_refresh {
        auth.refresh_token.clone()
    } else {
        String::new()
    };
    match vendor {
        OAuthVendor::Codex => {
            json!({"auth_mode":"chatgpt","OPENAI_API_KEY":Value::Null,"tokens":{"id_token":identity.id_token,"access_token":auth.access_token,"refresh_token":refresh,"account_id":identity.account_id},"last_refresh":iso(identity.updated_at)})
        }
        OAuthVendor::Claude => {
            json!({"claudeAiOauth":{"accessToken":auth.access_token,"refreshToken":refresh,"expiresAt":auth.access_token_expires_at.map(|at|at*1000),"scopes":identity.scopes,"subscriptionType":identity.plan}})
        }
        OAuthVendor::Gemini => {
            json!({"access_token":auth.access_token,"refresh_token":refresh,"id_token":identity.id_token,"scope":identity.scopes.join(" "),"token_type":"Bearer","expiry_date":auth.access_token_expires_at.map(|at|at*1000)})
        }
        OAuthVendor::Grok => {
            json!({format!("https://auth.x.ai::{}",identity.client_id):{"key":auth.access_token,"refresh_token":refresh,"expires_at":iso(auth.access_token_expires_at),"id_token":identity.id_token}})
        }
    }
}

/// The access tokens an agent home stores for this vendor's own login. A switch
/// made here can be recognized again by matching them against the cards'
/// credentials. Grok keys one entry per issuer and client.
pub(crate) fn stored_access_tokens(vendor: OAuthVendor, document: &Value) -> Vec<&str> {
    let pointer = |path: &str| document.pointer(path).and_then(Value::as_str);
    match vendor {
        OAuthVendor::Codex => pointer("/tokens/access_token").into_iter().collect(),
        OAuthVendor::Claude => pointer("/claudeAiOauth/accessToken").into_iter().collect(),
        OAuthVendor::Gemini => pointer("/access_token").into_iter().collect(),
        // Only the vendor scopes hold a login; a relay record under
        // `xai::api_key` holds a key of its own.
        OAuthVendor::Grok => document
            .as_object()
            .map(|root| {
                root.iter()
                    .filter(|(scope, _)| scope.starts_with("https://auth.x.ai::"))
                    .filter_map(|(_, entry)| entry.get("key").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Rejects the credential shapes the target CLI would misread, so a launch or
/// a native switch fails with a readable reason instead of a broken login.
pub(crate) fn unavailable_reason(
    vendor: OAuthVendor,
    auth: &ProviderAuth,
    identity: &OAuthCredential,
) -> Option<&'static str> {
    if auth.access_token.is_empty() {
        return Some("此账号只有刷新令牌，请先由本机续期取得访问令牌");
    }
    if identity.refresh_pending {
        return Some("账号正在续期或上次续期结果未确认，请完成授权后再使用原生授权");
    }
    if vendor == OAuthVendor::Codex
        && (identity.id_token.is_empty() || identity.account_id.is_empty())
    {
        return Some("Codex 原生授权需要完整的 ID Token 和账号工作区，请先重新授权");
    }
    if vendor == OAuthVendor::Claude
        && (auth.access_token_expires_at.is_none() || identity.scopes.is_empty())
    {
        return Some("Claude Code 原生授权需要到期时间和授权范围，请先重新授权");
    }
    None
}

pub(crate) fn iso(at: Option<i64>) -> Value {
    at.and_then(|at| chrono::DateTime::from_timestamp(at, 0))
        .map(|at| Value::String(at.to_rfc3339()))
        .unwrap_or(Value::Null)
}
