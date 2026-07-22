use crate::{
    adapters::newapi::NewApiAdapter,
    models::{AuthMode, Provider},
};

use super::{find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn change_password(
        &self,
        id: String,
        original_password: String,
        password: String,
    ) -> Result<String, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let message = NewApiAdapter
            .change_password(&data.settings, &provider, &original_password, &password)
            .await?;

        // Cookie / 访问令牌也代表同一个用户账号。只要已经有登录账号，
        // 就把新密码缓存下来，后续切换到账号密码模式时可以直接使用。
        if !supports_login_password_sync(&provider) {
            return Ok(message);
        }

        let new_password = password.trim().to_string();
        let synced = self
            .mutate(|data| {
                data.providers
                    .iter_mut()
                    .find(|stored| stored.identity.id == id)
                    .is_some_and(|stored| {
                        sync_password_if_context_unchanged(stored, &provider, &new_password)
                    })
            })
            .map_err(|error| format!("站点密码已更新，但本地登录密码保存失败：{error}"))?;

        if synced {
            Ok(message)
        } else {
            Ok(format!(
                "{message}；本地账号密码配置已发生变化，未覆盖现有登录密码"
            ))
        }
    }
}

fn sync_password_if_context_unchanged(
    stored: &mut Provider,
    snapshot: &Provider,
    new_password: &str,
) -> bool {
    if !supports_login_password_sync(snapshot)
        || !supports_login_password_sync(stored)
        || stored.identity.base_url != snapshot.identity.base_url
        || stored.auth.login_username != snapshot.auth.login_username
        || stored.auth.login_password != snapshot.auth.login_password
    {
        return false;
    }

    stored.auth.login_password = new_password.to_string();
    true
}

fn supports_login_password_sync(provider: &Provider) -> bool {
    matches!(
        provider.auth.mode,
        AuthMode::Password | AuthMode::Session | AuthMode::AccessToken
    ) && !provider.auth.login_username.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderIdentityInput, ProviderInput};

    fn provider(mode: AuthMode) -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    base_url: "https://relay.example.com".to_string(),
                    ..ProviderIdentityInput::default()
                },
                auth: crate::models::ProviderAuth {
                    mode,
                    login_username: "alice".to_string(),
                    login_password: "old-password".to_string(),
                    ..ProviderInput::default().auth
                },
                ..ProviderInput::default()
            },
            "provider-test".to_string(),
        )
    }

    #[test]
    fn password_change_syncs_matching_password_mode_context() {
        let snapshot = provider(AuthMode::Password);
        let mut stored = snapshot.clone();

        assert!(sync_password_if_context_unchanged(
            &mut stored,
            &snapshot,
            "new-password"
        ));
        assert_eq!(stored.auth.login_password, "new-password");
    }

    #[test]
    fn password_change_syncs_cookie_mode_login_credentials() {
        let snapshot = provider(AuthMode::Session);
        let mut stored = snapshot.clone();

        assert!(sync_password_if_context_unchanged(
            &mut stored,
            &snapshot,
            "new-password"
        ));
        assert_eq!(stored.auth.login_password, "new-password");
    }

    #[test]
    fn password_change_syncs_access_token_mode_login_credentials() {
        let snapshot = provider(AuthMode::AccessToken);
        let mut stored = snapshot.clone();

        assert!(sync_password_if_context_unchanged(
            &mut stored,
            &snapshot,
            "new-password"
        ));
        assert_eq!(stored.auth.login_password, "new-password");
    }

    #[test]
    fn password_change_does_not_write_without_login_username() {
        let mut snapshot = provider(AuthMode::Session);
        snapshot.auth.login_username.clear();
        let mut stored = snapshot.clone();

        assert!(!sync_password_if_context_unchanged(
            &mut stored,
            &snapshot,
            "new-password"
        ));
        assert_eq!(stored.auth.login_password, "old-password");
    }

    #[test]
    fn password_change_does_not_write_api_key_authentication() {
        let snapshot = provider(AuthMode::ApiKey);
        let mut stored = snapshot.clone();

        assert!(!sync_password_if_context_unchanged(
            &mut stored,
            &snapshot,
            "new-password"
        ));
        assert_eq!(stored.auth.login_password, "old-password");
    }

    #[test]
    fn password_change_does_not_overwrite_concurrent_credentials() {
        let snapshot = provider(AuthMode::Password);
        let mut stored = snapshot.clone();
        stored.auth.login_password = "manually-edited".to_string();

        assert!(!sync_password_if_context_unchanged(
            &mut stored,
            &snapshot,
            "new-password"
        ));
        assert_eq!(stored.auth.login_password, "manually-edited");
    }
}
