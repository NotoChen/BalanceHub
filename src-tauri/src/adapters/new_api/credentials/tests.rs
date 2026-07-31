use super::*;
use crate::models::{AuthMode, ProviderProxyMode};

const SYNTHETIC_SESSION_COOKIE: &str =
    "MHxBUVp6ZEhKcGJtY0FBQUFDYVdRRGFXNTBBQUFBL21CeXxzeW50aGV0aWMtc2lnbmF0dXJl";

fn provider_input_with_session(api_user: &str) -> ProviderInput {
    ProviderInput {
        identity: crate::models::ProviderIdentityInput {
            name: "Relay".to_string(),
            base_url: "https://relay.example.com".to_string(),
            ..crate::models::ProviderIdentityInput::default()
        },
        auth: crate::models::ProviderAuth {
            mode: AuthMode::Session,
            session_cookie: SYNTHETIC_SESSION_COOKIE.to_string(),
            api_user: api_user.to_string(),
            ..ProviderInput::default().auth
        },
        proxy: crate::models::ProviderProxy {
            mode: ProviderProxyMode::Inherit,
            url: String::new(),
        },
        ..ProviderInput::default()
    }
}

#[test]
fn session_cookie_user_id_overrides_stale_api_user() {
    let mut input = provider_input_with_session("stale-user");

    assert!(fill_api_user_from_session_cookie(&mut input));
    assert_eq!(input.auth.api_user, "12345");
}

#[test]
fn session_cookie_user_id_keeps_matching_api_user() {
    let mut input = provider_input_with_session("12345");

    assert!(!fill_api_user_from_session_cookie(&mut input));
    assert_eq!(input.auth.api_user, "12345");
}

#[test]
fn login_username_completion_prefers_username_over_email() {
    let mut input = provider_input_with_session("12345");
    let data = serde_json::json!({
        "username": "alice",
        "email": "alice@example.com",
    });

    assert!(fill_login_username_from_self(&mut input, &data));
    assert_eq!(input.auth.login_username, "alice");
}

#[test]
fn login_username_completion_falls_back_to_email() {
    let mut input = provider_input_with_session("12345");
    let data = serde_json::json!({"email": "alice@example.com"});

    assert!(fill_login_username_from_self(&mut input, &data));
    assert_eq!(input.auth.login_username, "alice@example.com");
}

#[test]
fn login_username_completion_preserves_manual_value() {
    let mut input = provider_input_with_session("12345");
    input.auth.login_username = "manual-account".to_string();
    let data = serde_json::json!({"username": "alice", "email": "alice@example.com"});

    assert!(!fill_login_username_from_self(&mut input, &data));
    assert_eq!(input.auth.login_username, "manual-account");
}
