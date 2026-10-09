use crate::{
    limits,
    models::{
        normalize_provider_endpoint, AppData, AuthMode, AuthSource, BrowserLoginBinding,
        BrowserLoginMechanism, LoginAccount, LoginPlatform, NewApiSession, Provider, ProviderInput,
    },
    services::provider_browser_login::LoginCredentials,
    util::unix_millis,
};

use super::{MutationDecision, ProviderRequestContext, ProviderService};

impl ProviderService<'_> {
    pub(in crate::services) async fn import_browser_login(
        &self,
        input: ProviderInput,
        expected: Option<Provider>,
        credentials: LoginCredentials,
        account: LoginAccount,
    ) -> Result<Provider, String> {
        self.mutate_decided_async(move |data| {
            import_credentials(data, input, expected.as_ref(), credentials, &account)
                .map(MutationDecision::changed)
        })
        .await
    }

    pub(in crate::services) async fn save_check_in_login(
        &self,
        expected: Provider,
        credentials: LoginCredentials,
        account: LoginAccount,
    ) -> Result<Provider, String> {
        self.mutate_decided_async(move |data| {
            save_check_in_credentials(data, &expected, credentials, &account)
                .map(MutationDecision::changed)
        })
        .await
    }
}

fn verify_login_account(
    data: &AppData,
    credentials: &LoginCredentials,
    account: &LoginAccount,
) -> Result<(), String> {
    let saved_account = data
        .login_accounts
        .iter()
        .find(|a| a.id == account.id && a.generation == account.generation)
        .ok_or("所选登录账号已更改，请重新登录")?;
    super::login_accounts::verify_identity(
        saved_account,
        credentials.platform,
        credentials.platform_identity.as_deref(),
    )
}

fn verify_site_user(provider: &Provider, credentials: &LoginCredentials) -> Result<(), String> {
    for previous in [&provider.identity.user_id, &provider.auth.api_user] {
        if !previous.trim().is_empty() && previous.trim() != credentials.user.id {
            return Err("登录的账号与该中转站卡片不一致，请使用原账号，或新增中转站导入".into());
        }
    }
    Ok(())
}

fn save_check_in_credentials(
    data: &mut AppData,
    expected: &Provider,
    credentials: LoginCredentials,
    account: &LoginAccount,
) -> Result<Provider, String> {
    if !credentials.fresh_login {
        return Err("未确认本次重新登录，不能将已有登录状态记为签到".into());
    }
    verify_login_account(data, &credentials, account)?;
    let context = ProviderRequestContext::capture(expected);
    let index = data
        .providers
        .iter()
        .position(|provider| provider.runtime.enabled && context.matches_check_in(provider))
        .ok_or("账号配置已变更，本次登录结果未写入")?;
    let provider = &data.providers[index];
    if provider
        .auth
        .browser_binding
        .as_ref()
        .and_then(|binding| binding.account_id.as_deref())
        != Some(account.id.as_str())
    {
        return Err("绑定的登录账号已变更，本次登录结果未写入".into());
    }
    verify_site_user(provider, &credentials)?;
    apply_credentials(data, index, credentials, account)
}

fn import_credentials(
    data: &mut AppData,
    input: ProviderInput,
    expected: Option<&Provider>,
    credentials: LoginCredentials,
    account: &LoginAccount,
) -> Result<Provider, String> {
    verify_login_account(data, &credentials, account)?;
    let endpoint = normalize_provider_endpoint(&input.identity.base_url);
    let target = if let Some(expected) = expected {
        let index = data
            .providers
            .iter()
            .position(|provider| provider.identity.id == expected.identity.id)
            .ok_or("中转站已删除，已取消登录导入")?;
        let stored = &data.providers[index];
        if login_configuration(stored)? != login_configuration(expected)? {
            return Err("中转站配置已更新，本次登录没有覆盖现有配置，请重试".to_string());
        }
        verify_site_user(stored, &credentials)?;
        Some(index)
    } else {
        data.providers.iter().position(|provider| {
            provider.identity.protocol == input.identity.protocol
                && normalize_provider_endpoint(&provider.identity.base_url) == endpoint
                && (provider.auth.api_user == credentials.user.id
                    || provider.identity.user_id == credentials.user.id)
        })
    };
    let index = if let Some(index) = target {
        // Reconnecting a duplicate only updates credentials; its card settings
        // are already authoritative. An existing editor may apply its own draft.
        if expected.is_some() {
            data.providers[index].apply_input(input);
        }
        index
    } else {
        if data.providers.len() >= limits::MAX_PROVIDERS {
            return Err(format!(
                "中转站数量已达到上限（{} 个）",
                limits::MAX_PROVIDERS
            ));
        }
        let mut input = input;
        input.auth = ProviderInput::default().auth;
        data.providers.push(Provider::from_input(
            input,
            format!("provider-{}", unix_millis()),
        ));
        data.providers.len() - 1
    };
    apply_credentials(data, index, credentials, account)
}

fn apply_credentials(
    data: &mut AppData,
    index: usize,
    credentials: LoginCredentials,
    account: &LoginAccount,
) -> Result<Provider, String> {
    let provider = &mut data.providers[index];
    let keep_login_check_in = provider.auth.mode == AuthMode::Password
        && !provider.auth.login_password.is_empty()
        && crate::models::provider_domain::check_in::effective_method(provider)
            == crate::models::ProviderCheckInMethod::FreshLogin;
    provider.auth.mode = if keep_login_check_in {
        AuthMode::Password
    } else {
        AuthMode::Session
    };
    provider.auth.source = if keep_login_check_in {
        AuthSource::Password
    } else if credentials.mechanism == BrowserLoginMechanism::Oauth {
        AuthSource::Oauth
    } else {
        AuthSource::Manual
    };
    provider.auth.session_cookie = if credentials.refresh_cookie.is_empty() {
        credentials.cookie_header
    } else {
        String::new()
    };
    provider.auth.api_user = credentials.user.id.clone();
    provider.auth.browser_binding = Some(BrowserLoginBinding {
        account_id: Some(account.id.clone()),
        platform: credentials.platform,
        mechanism: credentials.mechanism,
        imported_at: unix_millis() as i64,
    });
    provider.auth.credential_revision += 1;
    provider.auth.session_updated_at = Some(unix_millis() as i64);
    provider.auth.new_api_session = if credentials.refresh_cookie.is_empty() {
        None
    } else {
        Some(NewApiSession {
            refresh_cookie: credentials.refresh_cookie,
            access_token: credentials.access_token,
            access_expires_at: credentials.access_expires_at,
            session_id: credentials.session_id,
        })
    };
    provider.identity.user_id = credentials.user.id;
    provider.identity.username = credentials.user.username.clone();
    provider.identity.display_name = credentials.user.display_name;
    provider.auth.login_username = credentials.user.username;
    provider.runtime.error_message = None;
    let result = provider.clone();
    let saved_account = data
        .login_accounts
        .iter_mut()
        .find(|a| a.id == account.id)
        .ok_or("登录账号已不存在")?;
    saved_account.last_used_at = Some(unix_millis() as i64);
    if saved_account.platform == LoginPlatform::Unknown
        && credentials.platform != LoginPlatform::Unknown
    {
        saved_account.platform = credentials.platform;
    }
    if credentials.platform_identity.is_some() {
        saved_account.identity = credentials.platform_identity;
        saved_account.identity_observed_at = Some(unix_millis() as i64);
    }
    Ok(result)
}

/// ProviderInput is the canonical editable configuration. Discard observation
/// fields and server-owned session rotation when comparing long-running login
/// drafts, so an unrelated background refresh cannot invalidate a login.
fn login_configuration(provider: &Provider) -> Result<serde_json::Value, String> {
    let value = serde_json::to_value(provider).map_err(|_| "无法核对中转站配置")?;
    let mut input: ProviderInput =
        serde_json::from_value(value).map_err(|_| "无法核对中转站配置")?;
    input.identity.name.clear();
    input.identity.user_id.clear();
    input.auth.new_api_session = None;
    input.auth.session_updated_at = None;
    input.auth.api_key_options.clear();
    input.auth.api_key_token_id.clear();
    if input.auth.mode == AuthMode::Password {
        input.auth.session_cookie.clear();
        input.auth.api_user.clear();
    }
    serde_json::to_value(input).map_err(|_| "无法核对中转站配置".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::provider_browser_login::LoginUser;

    fn credentials(id: &str) -> LoginCredentials {
        LoginCredentials {
            cookie_header: "session=fixture".into(),
            access_token: String::new(),
            access_expires_at: None,
            refresh_cookie: String::new(),
            session_id: String::new(),
            user: LoginUser {
                id: id.into(),
                username: "fixture-user".into(),
                display_name: String::new(),
            },
            mechanism: BrowserLoginMechanism::Unknown,
            platform: LoginPlatform::Unknown,
            platform_identity: None,
            fresh_login: false,
        }
    }

    fn import_credentials(
        data: &mut AppData,
        input: ProviderInput,
        expected: Option<&Provider>,
        credentials: LoginCredentials,
    ) -> Result<Provider, String> {
        let account = LoginAccount::new(
            "account-fixture".into(),
            "Fixture".into(),
            LoginPlatform::Unknown,
            1,
        );
        if data.login_accounts.is_empty() {
            data.login_accounts.push(account.clone());
        }
        super::import_credentials(data, input, expected, credentials, &account)
    }

    #[test]
    fn login_rejects_a_changed_card_or_another_account() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://example.test".into();
        input.auth.api_user = "42".into();
        let expected = Provider::from_input(input.clone(), "fixture".into());
        let mut data = AppData::default();
        data.providers.push(expected.clone());
        assert!(
            import_credentials(&mut data, input.clone(), Some(&expected), credentials("43"))
                .is_err()
        );
        data.providers[0].identity.remark = "edited-during-login".into();
        assert!(import_credentials(&mut data, input, Some(&expected), credentials("42")).is_err());
        assert!(data.providers[0].auth.session_cookie.is_empty());
    }

    #[test]
    fn background_observations_do_not_invalidate_a_login_draft() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://example.test".into();
        input.auth.api_user = "42".into();
        let expected = Provider::from_input(input.clone(), "fixture".into());
        let mut current = expected.clone();
        current.revision = 22;
        current.quota.available = 100.0;
        current.identity.name = "Remote name".into();
        current.automation.last_synced_at = Some("123".into());
        current.auth.session_updated_at = Some(123);
        let mut data = AppData::default();
        data.providers.push(current);
        assert!(import_credentials(&mut data, input, Some(&expected), credentials("42")).is_ok());
    }

    #[test]
    fn duplicate_login_updates_credentials_without_replacing_card_preferences() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://example.test".into();
        input.auth.api_user = "42".into();
        let mut existing = Provider::from_input(input.clone(), "fixture".into());
        existing.identity.remark = "keep-remark".into();
        let mut data = AppData::default();
        data.providers.push(existing);
        let result = import_credentials(&mut data, input, None, credentials("42")).unwrap();
        assert_eq!(data.providers.len(), 1);
        assert_eq!(result.identity.remark, "keep-remark");
        assert_eq!(result.auth.session_cookie, "session=fixture");
    }

    #[test]
    fn imports_bind_each_station_to_its_selected_account_and_reject_cleared_accounts() {
        let mut data = AppData::default();
        let a = LoginAccount::new("a".into(), "A".into(), LoginPlatform::LinuxDo, 1);
        let b = LoginAccount::new("b".into(), "B".into(), LoginPlatform::LinuxDo, 1);
        data.login_accounts = vec![a.clone(), b.clone()];
        for (account, site, user) in [
            (&a, "https://xxx1.test", "1"),
            (&b, "https://xxx2.test", "2"),
            (&a, "https://yyy1.test", "3"),
        ] {
            let mut input = ProviderInput::default();
            input.identity.base_url = site.into();
            let mut creds = credentials(user);
            creds.platform = LoginPlatform::LinuxDo;
            creds.mechanism = BrowserLoginMechanism::Oauth;
            creds.platform_identity = Some(format!("platform-{}", account.id));
            let result = super::import_credentials(&mut data, input, None, creds, account).unwrap();
            assert_eq!(
                result.auth.browser_binding.unwrap().account_id.as_deref(),
                Some(account.id.as_str())
            );
        }
        assert_eq!(data.providers.len(), 3);
        assert_eq!(
            data.login_accounts[0].identity.as_deref(),
            Some("platform-a")
        );
        assert_eq!(
            data.login_accounts[1].identity.as_deref(),
            Some("platform-b")
        );
        let mut wrong = credentials("4");
        wrong.platform = LoginPlatform::LinuxDo;
        wrong.platform_identity = Some("platform-b".into());
        assert!(
            super::import_credentials(&mut data, ProviderInput::default(), None, wrong, &a)
                .is_err()
        );
        data.login_accounts[0].generation += 1;
        assert!(super::import_credentials(
            &mut data,
            ProviderInput::default(),
            None,
            credentials("5"),
            &a
        )
        .is_err());
        assert_eq!(data.providers.len(), 3);
    }

    fn check_in_fixture() -> (AppData, Provider, LoginAccount, LoginCredentials) {
        let mut account = LoginAccount::new(
            "fixture-account".into(),
            "Fixture".into(),
            LoginPlatform::LinuxDo,
            1,
        );
        account.identity = Some("fixture-identity".into());
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://agentrouter.org".into();
        input.auth.mode = AuthMode::Session;
        input.auth.source = AuthSource::Oauth;
        input.auth.api_user = "42".into();
        input.auth.session_cookie = "session=old-fixture".into();
        input.auth.browser_binding = Some(BrowserLoginBinding {
            account_id: Some(account.id.clone()),
            platform: LoginPlatform::LinuxDo,
            mechanism: BrowserLoginMechanism::Oauth,
            imported_at: 1,
        });
        let mut provider = Provider::from_input(input, "fixture".into());
        provider.identity.remark = "keep-card-preferences".into();
        let mut login = credentials("42");
        login.mechanism = BrowserLoginMechanism::Oauth;
        login.platform = LoginPlatform::LinuxDo;
        login.platform_identity = account.identity.clone();
        login.fresh_login = true;
        let data = AppData {
            providers: vec![provider.clone()],
            login_accounts: vec![account.clone()],
            ..AppData::default()
        };
        (data, provider, account, login)
    }

    #[test]
    fn check_in_login_updates_bound_credentials_without_applying_an_editor_draft() {
        let (mut data, expected, account, login) = check_in_fixture();
        data.providers[0].identity.remark = "edited-while-logging-in".into();
        let provider = save_check_in_credentials(&mut data, &expected, login, &account).unwrap();
        assert_eq!(data.providers.len(), 1);
        assert_eq!(provider.identity.remark, "edited-while-logging-in");
        assert_eq!(provider.auth.mode, AuthMode::Session);
        assert_eq!(provider.auth.source, AuthSource::Oauth);
        assert_eq!(provider.auth.session_cookie, "session=fixture");
        assert_eq!(
            provider.auth.credential_revision,
            expected.auth.credential_revision + 1
        );
        assert_eq!(
            provider.auth.browser_binding.unwrap().account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert!(data.login_accounts[0].last_used_at.is_some());
        assert!(
            provider.automation.last_checked_in_at.is_none(),
            "the common check-in finalizer owns records"
        );
    }

    #[test]
    fn check_in_login_rejects_stale_configuration_unproven_login_and_changed_identity() {
        for scenario in [
            "no-login",
            "site-user",
            "platform-user",
            "cleared-account",
            "binding",
            "detached-binding",
            "removed-binding",
            "policy",
            "credential",
            "disabled",
            "deleted",
        ] {
            let (mut data, expected, account, mut login) = check_in_fixture();
            match scenario {
                "no-login" => login.fresh_login = false,
                "site-user" => login.user.id = "43".into(),
                "platform-user" => {
                    login.platform_identity = Some("another-fixture-identity".into())
                }
                "cleared-account" => data.login_accounts[0].generation += 1,
                "binding" => {
                    data.providers[0]
                        .auth
                        .browser_binding
                        .as_mut()
                        .unwrap()
                        .account_id = Some("other-account".into())
                }
                "detached-binding" => {
                    data.providers[0]
                        .auth
                        .browser_binding
                        .as_mut()
                        .unwrap()
                        .account_id = None
                }
                "removed-binding" => data.providers[0].auth.browser_binding = None,
                "policy" => {
                    data.providers[0].automation.check_in_method =
                        crate::models::ProviderCheckInMethod::Standard
                }
                "credential" => data.providers[0].auth.credential_revision += 1,
                "disabled" => data.providers[0].runtime.enabled = false,
                "deleted" => data.providers.clear(),
                _ => unreachable!(),
            }
            let before = serde_json::to_value(&data).unwrap();
            assert!(
                save_check_in_credentials(&mut data, &expected, login, &account).is_err(),
                "{scenario}"
            );
            assert_eq!(
                serde_json::to_value(&data).unwrap(),
                before,
                "{scenario} must not write partial credentials"
            );
        }
    }
}
