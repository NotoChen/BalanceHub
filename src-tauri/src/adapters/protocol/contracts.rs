use crate::models::{
    AppSettings, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderCredentialCompletionResult, ProviderInput, ProviderQuota, ProviderRequestLogsQuery,
    ProviderRequestLogsResult, ProviderSiteProbeResult, ProviderStatus, ProviderUsageSummary,
    SiteAnnouncement,
};
use async_trait::async_trait;

/// 认证请求可能产生的最小凭据变更。
///
/// Adapter 不再把整份 `Provider` 返回给业务层，避免一次令牌轮换顺带覆盖配额、能力、
/// 自动化配置或运行状态。Service 仍会在完整 `ProviderRequestContext` CAS 校验通过后应用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderCredentialPatch {
    session_cookie: Option<String>,
    api_user: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    access_token_expires_at: Option<Option<i64>>,
    login_username: Option<String>,
}

impl ProviderCredentialPatch {
    fn from_authenticated(original: &Provider, authenticated: &Provider) -> Self {
        let mut patch = Self::default();
        match original.identity.protocol {
            crate::models::ProviderProtocol::Sub2Api => {
                patch.access_token = Some(authenticated.auth.access_token.clone());
                patch.refresh_token = Some(authenticated.auth.refresh_token.clone());
                patch.access_token_expires_at = Some(authenticated.auth.access_token_expires_at);
                if original.auth.login_username.trim().is_empty()
                    && !authenticated.auth.login_username.trim().is_empty()
                {
                    patch.login_username = Some(authenticated.auth.login_username.clone());
                }
            }
            crate::models::ProviderProtocol::NewApi => {
                if !authenticated.auth.session_cookie.trim().is_empty() {
                    patch.session_cookie = Some(authenticated.auth.session_cookie.clone());
                }
                if !authenticated.auth.api_user.trim().is_empty() {
                    patch.api_user = Some(authenticated.auth.api_user.clone());
                }
                if !authenticated.auth.access_token.trim().is_empty() {
                    patch.access_token = Some(authenticated.auth.access_token.clone());
                }
                if original.auth.login_username.trim().is_empty()
                    && !authenticated.auth.login_username.trim().is_empty()
                {
                    patch.login_username = Some(authenticated.auth.login_username.clone());
                }
            }
            crate::models::ProviderProtocol::Api => {}
        }
        patch
    }

    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn apply(&self, provider: &mut Provider) -> bool {
        let previous = provider.auth.clone();
        apply_string(&mut provider.auth.session_cookie, &self.session_cookie);
        apply_string(&mut provider.auth.api_user, &self.api_user);
        apply_string(&mut provider.auth.access_token, &self.access_token);
        apply_string(&mut provider.auth.refresh_token, &self.refresh_token);
        if let Some(expires_at) = self.access_token_expires_at {
            provider.auth.access_token_expires_at = expires_at;
        }
        apply_string(&mut provider.auth.login_username, &self.login_username);
        provider.auth != previous
    }
}

fn apply_string(target: &mut String, value: &Option<String>) {
    if let Some(value) = value {
        target.clone_from(value);
    }
}

/// 刷新流程拥有的远端观察数据，不包含用户配置。
#[derive(Debug, Clone)]
pub(crate) struct ProviderObservationPatch {
    identity_name: String,
    display_name: String,
    username: String,
    user_id: String,
    site_logo: String,
    quota: ProviderQuota,
    available_models: Option<Vec<String>>,
    last_synced_at: Option<String>,
    status: ProviderStatus,
    error_message: Option<String>,
}

impl ProviderObservationPatch {
    fn from_refreshed(refreshed: &Provider) -> Self {
        Self {
            identity_name: refreshed.identity.name.clone(),
            display_name: refreshed.identity.display_name.clone(),
            username: refreshed.identity.username.clone(),
            user_id: refreshed.identity.user_id.clone(),
            site_logo: refreshed.identity.site_logo.clone(),
            quota: refreshed.quota.clone(),
            available_models: (!matches!(refreshed.runtime.status, ProviderStatus::Error))
                .then(|| refreshed.capabilities.available_models.clone()),
            last_synced_at: refreshed.automation.last_synced_at.clone(),
            status: refreshed.runtime.status,
            error_message: refreshed.runtime.error_message.clone(),
        }
    }

    pub(crate) fn apply(&self, provider: &mut Provider) {
        provider.identity.name.clone_from(&self.identity_name);
        provider
            .identity
            .display_name
            .clone_from(&self.display_name);
        provider.identity.username.clone_from(&self.username);
        provider.identity.user_id.clone_from(&self.user_id);
        provider.identity.site_logo.clone_from(&self.site_logo);
        provider.quota.clone_from(&self.quota);
        if let Some(models) = &self.available_models {
            provider.capabilities.available_models.clone_from(models);
        }
        provider
            .automation
            .last_synced_at
            .clone_from(&self.last_synced_at);
        provider.runtime.status = self.status;
        provider
            .runtime
            .error_message
            .clone_from(&self.error_message);
    }
}

/// 业务结果及其显式认证副作用。
#[derive(Debug)]
pub(crate) struct ProviderOperationOutcome<T> {
    pub(crate) credentials: ProviderCredentialPatch,
    pub(crate) observation: Option<ProviderObservationPatch>,
    pub(crate) value: T,
}

impl<T> ProviderOperationOutcome<T> {
    pub(crate) fn from_authenticated_result(
        original: &Provider,
        (authenticated, value): (Provider, T),
    ) -> Self {
        Self::authenticated(original, authenticated, value)
    }

    pub(crate) fn authenticated(original: &Provider, authenticated: Provider, value: T) -> Self {
        Self {
            credentials: ProviderCredentialPatch::from_authenticated(original, &authenticated),
            observation: None,
            value,
        }
    }

    pub(crate) fn unchanged(value: T) -> Self {
        Self {
            credentials: ProviderCredentialPatch::default(),
            observation: None,
            value,
        }
    }

    pub(crate) fn refreshed(original: &Provider, refreshed: Provider) -> Self
    where
        T: Default,
    {
        Self {
            credentials: ProviderCredentialPatch::from_authenticated(original, &refreshed),
            observation: Some(ProviderObservationPatch::from_refreshed(&refreshed)),
            value: T::default(),
        }
    }

    pub(crate) fn apply_to(&self, provider: &mut Provider) {
        self.credentials.apply(provider);
        if let Some(observation) = &self.observation {
            observation.apply(provider);
        }
    }
}

#[async_trait]
pub(crate) trait CredentialCapability: Send + Sync {
    async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String>;
}

#[async_trait]
pub(crate) trait AccessTokenCapability: Send + Sync {
    async fn generate_access_token(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String>;
}

#[async_trait]
pub(crate) trait ConnectionCapability: Send + Sync {
    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String>;

    async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String>;

    async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()>;
}

#[async_trait]
pub(crate) trait ApiKeyManagementCapability: Send + Sync {
    async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<ProviderApiKeyOption>>, String>;

    async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderOperationOutcome<ProviderApiKeyOption>, String>;

    async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String>;
}

#[async_trait]
pub(crate) trait UsageCapability: Send + Sync {
    async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderOperationOutcome<ProviderUsageSummary>, String>;

    async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderOperationOutcome<ProviderRequestLogsResult>, String>;
}

#[async_trait]
pub(crate) trait AccountCapability: Send + Sync {
    async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<ProviderOperationOutcome<String>, String>;

    async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<String>, String>;
}

#[async_trait]
pub(crate) trait CapabilityProbe: Send + Sync {
    async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>;
}

#[async_trait]
pub(crate) trait CheckInCapability: Send + Sync {
    async fn check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, String>;

    async fn check_in_records(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        month: &str,
    ) -> Result<ProviderOperationOutcome<ProviderCheckInRecordsResult>, String>;
}

#[async_trait]
pub(crate) trait AnnouncementCapability: Send + Sync {
    async fn list_announcements(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<Vec<SiteAnnouncement>>, String>;

    async fn mark_announcement_read(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        announcement_id: &str,
    ) -> Result<ProviderOperationOutcome<()>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderProtocol};

    fn provider(protocol: ProviderProtocol) -> Provider {
        let mut input = ProviderInput::default();
        input.identity.protocol = protocol;
        Provider::from_input(input, "provider-1".to_string())
    }

    #[test]
    fn unchanged_operation_has_no_credential_write() {
        let original = provider(ProviderProtocol::NewApi);
        let outcome = ProviderOperationOutcome::unchanged(());

        assert!(outcome.credentials.is_empty());
        let mut stored = original.clone();
        assert!(!outcome.credentials.apply(&mut stored));
        assert_eq!(stored.auth, original.auth);
    }

    #[test]
    fn credential_patch_only_applies_authenticated_fields() {
        let original = provider(ProviderProtocol::NewApi);
        let mut authenticated = original.clone();
        authenticated.auth.session_cookie = "session=new".to_string();
        authenticated.auth.api_user = "42".to_string();
        authenticated.identity.name = "不应越层覆盖".to_string();
        authenticated.quota.available = 99.0;

        let outcome = ProviderOperationOutcome::authenticated(&original, authenticated, ());
        let mut stored = original.clone();
        outcome.apply_to(&mut stored);

        assert_eq!(stored.auth.session_cookie, "session=new");
        assert_eq!(stored.auth.api_user, "42");
        assert_eq!(stored.identity.name, original.identity.name);
        assert_eq!(stored.quota.available, original.quota.available);
    }

    #[test]
    fn sub2_credential_patch_can_clear_expired_tokens() {
        let mut original = provider(ProviderProtocol::Sub2Api);
        original.auth.access_token = "expired".to_string();
        original.auth.refresh_token = "consumed".to_string();
        original.auth.access_token_expires_at = Some(123);
        let mut authenticated = original.clone();
        authenticated.auth.access_token.clear();
        authenticated.auth.refresh_token.clear();
        authenticated.auth.access_token_expires_at = None;

        let outcome = ProviderOperationOutcome::authenticated(&original, authenticated, ());
        outcome.apply_to(&mut original);

        assert!(original.auth.access_token.is_empty());
        assert!(original.auth.refresh_token.is_empty());
        assert_eq!(original.auth.access_token_expires_at, None);
    }

    #[test]
    fn automatic_token_rotation_keeps_derived_capability_cache() {
        let mut original = provider(ProviderProtocol::Sub2Api);
        original.auth.access_token = "access-old".to_string();
        original.auth.refresh_token = "refresh-old".to_string();
        original.capabilities.available_models = vec!["cached-model".to_string()];
        original.capabilities.api_key_management_known = true;
        let mut authenticated = original.clone();
        authenticated.auth.access_token = "access-new".to_string();
        authenticated.auth.refresh_token = "refresh-new".to_string();

        ProviderOperationOutcome::authenticated(&original.clone(), authenticated, ())
            .apply_to(&mut original);

        assert_eq!(original.auth.access_token, "access-new");
        assert_eq!(original.auth.refresh_token, "refresh-new");
        assert_eq!(
            original.capabilities.available_models,
            vec!["cached-model".to_string()]
        );
        assert!(original.capabilities.api_key_management_known);
    }

    #[test]
    fn refresh_observation_does_not_replace_provider_configuration() {
        let mut original = provider(ProviderProtocol::Api);
        original.identity.base_url = "https://configured.example.com".to_string();
        original.capabilities.available_models = vec!["old-model".to_string()];
        let mut refreshed = original.clone();
        refreshed.identity.base_url = "https://unexpected.example.com".to_string();
        refreshed.identity.name = "远端站点名".to_string();
        refreshed.quota.available = 88.0;
        refreshed.capabilities.available_models = vec!["new-model".to_string()];
        refreshed.runtime.status = ProviderStatus::Ok;

        let outcome = ProviderOperationOutcome::<()>::refreshed(&original, refreshed);
        outcome.apply_to(&mut original);

        assert_eq!(original.identity.base_url, "https://configured.example.com");
        assert_eq!(original.identity.name, "远端站点名");
        assert_eq!(original.quota.available, 88.0);
        assert_eq!(
            original.capabilities.available_models,
            vec!["new-model".to_string()]
        );
    }

    #[test]
    fn failed_refresh_keeps_last_known_model_list() {
        let mut original = provider(ProviderProtocol::Api);
        original.capabilities.available_models = vec!["known-model".to_string()];
        let mut refreshed = original.clone();
        refreshed.capabilities.available_models.clear();
        refreshed.runtime.status = ProviderStatus::Error;

        ProviderOperationOutcome::<()>::refreshed(&original.clone(), refreshed)
            .apply_to(&mut original);

        assert_eq!(
            original.capabilities.available_models,
            vec!["known-model".to_string()]
        );
    }
}
