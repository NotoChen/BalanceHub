use super::*;
use crate::models::NewApiSession;

fn configured_provider(protocol: ProviderProtocol) -> Provider {
    let mut input = ProviderInput::default();
    input.identity.protocol = protocol;
    input.identity.base_url = "https://relay.example.invalid".into();
    input.auth.mode = AuthMode::AccessToken;
    input.auth.login_username = "fixture-user".into();
    input.auth.login_password = "fixture-password".into();
    input.auth.api_user = "42".into();
    input.auth.access_token = "fixture-access-token".into();
    input.auth.refresh_token = "fixture-refresh-token".into();
    input.auth.access_token_expires_at = Some(4102444800000);
    input.auth.api_key = "sk-fixture-key".into();
    if protocol == ProviderProtocol::NewApi {
        input.auth.mode = AuthMode::Session;
        input.auth.session_cookie = "session=fixture-cookie".into();
        input.auth.new_api_session = Some(NewApiSession {
            access_token: "fixture-session-token".into(),
            access_expires_at: Some(4102444800),
            refresh_cookie: "fixture-session-refresh".into(),
            session_id: "fixture-session".into(),
        });
    }
    Provider::from_input(input, "fixture-provider".into())
}

fn editable(provider: &Provider) -> ProviderInput {
    let mut input: ProviderInput =
        serde_json::from_value(serde_json::to_value(provider).unwrap()).unwrap();
    input.id = Some(provider.identity.id.clone());
    input
}

#[test]
fn auth_mode_switches_preserve_credentials_through_save_and_reload() {
    for protocol in [ProviderProtocol::NewApi, ProviderProtocol::Sub2Api] {
        let mut provider = configured_provider(protocol);
        let mut modes = vec![AuthMode::Password, AuthMode::AccessToken, AuthMode::ApiKey];
        if protocol == ProviderProtocol::NewApi {
            modes.push(AuthMode::Session);
        }
        modes.push(AuthMode::Password);
        for mode in modes {
            let mut expected = provider.auth.clone();
            expected.credential_revision += u64::from(expected.mode != mode);
            expected.mode = mode;
            let mut input = editable(&provider);
            input.auth.mode = mode;
            provider.apply_input(input);
            provider = serde_json::from_value(serde_json::to_value(provider).unwrap()).unwrap();
            assert_eq!(provider.auth, normalize_provider_auth(expected, protocol));
        }
    }
}

#[test]
fn auth_mode_switch_keeps_the_latest_backend_session_instead_of_the_old_draft() {
    let mut provider = configured_provider(ProviderProtocol::NewApi);
    let mut input = editable(&provider);
    let session = provider.auth.new_api_session.as_mut().unwrap();
    session.access_token = "fixture-rotated-token".into();
    session.refresh_cookie = "fixture-rotated-refresh".into();
    let expected = provider.auth.new_api_session.clone();
    input.auth.mode = AuthMode::AccessToken;
    provider.apply_input(input);
    assert_eq!(provider.auth.new_api_session, expected);
}

#[test]
fn changing_login_credentials_while_switching_modes_still_invalidates_the_old_login() {
    let mut provider = configured_provider(ProviderProtocol::NewApi);
    let mut input = editable(&provider);
    input.auth.mode = AuthMode::Password;
    input.auth.login_password = "fixture-new-password".into();
    provider.apply_input(input);
    assert!(provider.auth.session_cookie.is_empty());
    assert!(provider.auth.new_api_session.is_none());
    assert!(provider.auth.api_user.is_empty());
}
