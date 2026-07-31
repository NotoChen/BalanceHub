use chrono::{Datelike, Local};

use crate::models::{provider_domain::auth, AuthMode, Provider, ProviderProtocol};

pub fn supports_account_management(provider: &Provider) -> bool {
    if matches!(provider.identity.protocol, ProviderProtocol::Api)
        || matches!(provider.auth.mode, AuthMode::ApiKey)
    {
        return false;
    }

    if matches!(provider.identity.protocol, ProviderProtocol::Sub2Api) {
        return if matches!(provider.auth.mode, AuthMode::Password) {
            (!provider.auth.login_username.trim().is_empty()
                && !provider.auth.login_password.trim().is_empty())
                || auth::has_access_token(provider)
        } else {
            auth::has_access_token(provider)
        };
    }

    if matches!(provider.auth.mode, AuthMode::Password) {
        return (!provider.auth.login_username.trim().is_empty()
            && !provider.auth.login_password.trim().is_empty())
            || (auth::has_api_user(provider)
                && (auth::has_access_token(provider) || auth::has_session(provider)));
    }

    auth::has_api_user(provider)
        && (auth::has_access_token(provider) || auth::has_session(provider))
}

pub fn supports_check_in(provider: &Provider, is_anyrouter: bool) -> bool {
    if !matches!(provider.identity.protocol, ProviderProtocol::NewApi)
        || matches!(provider.auth.mode, AuthMode::ApiKey)
    {
        return false;
    }
    let capabilities = &provider.capabilities;
    if capabilities.check_in_known {
        return capabilities.check_in_supported;
    }
    if is_anyrouter {
        return auth::has_session(provider);
    }

    (matches!(provider.auth.mode, AuthMode::AccessToken)
        && auth::has_access_token(provider)
        && auth::has_api_user(provider))
        || (matches!(provider.auth.mode, AuthMode::Session)
            && auth::has_session(provider)
            && auth::has_api_user(provider))
        || (matches!(provider.auth.mode, AuthMode::Password)
            && auth::has_session(provider)
            && auth::has_api_user(provider))
}

pub fn supports_api_key_management(provider: &Provider) -> bool {
    if !supports_account_management(provider) {
        return false;
    }
    if provider.capabilities.api_key_management_known {
        return provider.capabilities.api_key_management_supported;
    }
    true
}

pub fn supports_invitation(provider: &Provider) -> bool {
    if !supports_account_management(provider) {
        return false;
    }
    if provider.capabilities.invitation_known {
        return provider.capabilities.invitation_supported;
    }
    !provider.capabilities.invite_link.trim().is_empty() || supports_account_management(provider)
}

pub fn check_in_user(provider: &Provider, is_anyrouter: bool) -> String {
    let api_user = provider.auth.api_user.trim();
    if !api_user.is_empty() {
        api_user.to_string()
    } else if is_anyrouter {
        provider.identity.id.clone()
    } else {
        String::new()
    }
}

pub fn checked_in_today(provider: &Provider, is_anyrouter: bool) -> bool {
    if !supports_check_in(provider, is_anyrouter) {
        return false;
    }
    let Some(checked_ymd) = local_ymd_from_stored(&provider.automation.last_checked_in_at) else {
        return false;
    };
    let now = Local::now();
    if checked_ymd != (now.year(), now.month(), now.day()) {
        return false;
    }
    let checked_user = provider.automation.last_check_in_user.trim();
    checked_user.is_empty() || checked_user == check_in_user(provider, is_anyrouter)
}

/// 把存储的「上次签到时刻」（毫秒/秒数字串，或 RFC3339）解析为本地年月日。
fn local_ymd_from_stored(value: &Option<String>) -> Option<(i32, u32, u32)> {
    let raw = value.as_ref()?.trim();
    if raw.is_empty() {
        return None;
    }
    let datetime = if let Ok(number) = raw.parse::<i128>() {
        let secs = if number > 1_000_000_000_000 {
            (number / 1000) as i64
        } else {
            number as i64
        };
        chrono::DateTime::from_timestamp(secs, 0)?.with_timezone(&Local)
    } else {
        chrono::DateTime::parse_from_rfc3339(raw)
            .ok()?
            .with_timezone(&Local)
    };
    Some((datetime.year(), datetime.month(), datetime.day()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderAuth, ProviderIdentityInput, ProviderInput};

    fn provider() -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    name: "站点".to_string(),
                    base_url: "https://example.com".to_string(),
                    ..ProviderIdentityInput::default()
                },
                auth: ProviderAuth {
                    mode: AuthMode::Session,
                    session_cookie: "session-value".to_string(),
                    api_user: "1001".to_string(),
                    ..ProviderInput::default().auth
                },
                ..ProviderInput::default()
            },
            "p1".to_string(),
        )
    }

    #[test]
    fn known_capability_overrides_credential_guess() {
        let mut provider = provider();
        provider.capabilities.check_in_known = true;
        provider.capabilities.check_in_supported = false;

        assert!(!supports_check_in(&provider, false));
    }

    #[test]
    fn api_key_mode_never_inherits_cached_check_in_capability() {
        let mut provider = provider();
        provider.auth.mode = AuthMode::ApiKey;
        provider.capabilities.check_in_known = true;
        provider.capabilities.check_in_supported = true;

        assert!(!supports_check_in(&provider, false));
    }

    #[test]
    fn account_and_remote_capabilities_follow_the_active_auth_mode() {
        let mut provider = provider();
        assert!(supports_account_management(&provider));
        assert!(supports_api_key_management(&provider));
        assert!(supports_invitation(&provider));

        provider.auth.mode = AuthMode::ApiKey;
        assert!(!supports_account_management(&provider));
        assert!(!supports_api_key_management(&provider));
        assert!(!supports_invitation(&provider));
    }

    #[test]
    fn non_newapi_protocols_never_infer_check_in_support() {
        let mut provider = provider();
        provider.identity.protocol = ProviderProtocol::Sub2Api;
        provider.auth.mode = AuthMode::AccessToken;
        provider.auth.access_token = "token".to_string();

        assert!(!supports_check_in(&provider, false));
    }

    #[test]
    fn anyrouter_uses_provider_id_when_user_is_unavailable() {
        let mut provider = provider();
        provider.auth.api_user.clear();

        assert_eq!(check_in_user(&provider, true), "p1");
        assert_eq!(check_in_user(&provider, false), "");
    }
}
