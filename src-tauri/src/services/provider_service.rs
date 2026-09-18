mod account;
mod announcements;
mod api_keys;
mod available_models;
mod browser_login;
mod login_accounts;
pub(crate) use login_accounts::{LoginAccountSummary, LoginCookieSummary};
mod capabilities;
mod check_in;
mod credential_details;
mod credentials;
pub(crate) use credential_details::{CredentialKind, ProviderCredentialDetails};
mod liveness;
mod persistence;
mod quota;
mod refresh;
mod transaction;
mod usage;
mod workspaces;

use crate::models::{
    AppData, AuthMode, AuthSource, Provider, ProviderCheckInMethod, ProviderProtocol,
    ProviderTurnstileMode,
};
use tauri::AppHandle;

use transaction::MutationDecision;

pub struct ProviderService<'a> {
    app: &'a AppHandle,
}

/// 异步 Provider 请求发出时的完整认证上下文。
///
/// 网络返回后必须再次与最新存储状态比对；只按 provider id 或 URL 判断，会让旧账号的
/// 身份、额度、签到、能力等结果写进用户刚切换的新账号。API Key 列表本身是派生元数据，
/// 不参与上下文判断，避免一次纯列表刷新无意义地取消同账号请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderRequestContext {
    provider_id: String,
    base_url: String,
    protocol: ProviderProtocol,
    auth_mode: AuthMode,
    auth_source: AuthSource,
    api_key: String,
    api_key_token_id: String,
    access_token: String,
    session_cookie: String,
    api_user: String,
    login_username: String,
    login_password: String,
    refresh_token: String,
    access_token_expires_at: Option<i64>,
    new_api_session: Option<crate::models::NewApiSession>,
    credential_revision: u64,
    check_in_method: ProviderCheckInMethod,
    auto_shield: bool,
    turnstile_mode: ProviderTurnstileMode,
}

impl ProviderRequestContext {
    pub(super) fn capture(provider: &Provider) -> Self {
        Self {
            provider_id: provider.identity.id.clone(),
            base_url: provider.identity.base_url.clone(),
            protocol: provider.identity.protocol,
            auth_mode: provider.auth.mode,
            auth_source: provider.auth.source,
            api_key: provider.auth.api_key.clone(),
            api_key_token_id: provider.auth.api_key_token_id.clone(),
            access_token: provider.auth.access_token.clone(),
            session_cookie: provider.auth.session_cookie.clone(),
            api_user: provider.auth.api_user.clone(),
            login_username: provider.auth.login_username.clone(),
            login_password: provider.auth.login_password.clone(),
            refresh_token: provider.auth.refresh_token.clone(),
            access_token_expires_at: provider.auth.access_token_expires_at,
            new_api_session: provider.auth.new_api_session.clone(),
            credential_revision: provider.auth.credential_revision,
            check_in_method: provider.automation.check_in_method,
            auto_shield: provider.automation.auto_shield,
            turnstile_mode: provider.automation.turnstile_mode,
        }
    }

    pub(super) fn matches(&self, provider: &Provider) -> bool {
        self.provider_id == provider.identity.id
            && self.base_url == provider.identity.base_url
            && self.protocol == provider.identity.protocol
            && self.auth_mode == provider.auth.mode
            && self.auth_source == provider.auth.source
            && self.api_key == provider.auth.api_key
            && self.api_key_token_id == provider.auth.api_key_token_id
            && self.access_token == provider.auth.access_token
            && self.session_cookie == provider.auth.session_cookie
            && self.api_user == provider.auth.api_user
            && self.login_username == provider.auth.login_username
            && self.login_password == provider.auth.login_password
            && self.refresh_token == provider.auth.refresh_token
            && self.access_token_expires_at == provider.auth.access_token_expires_at
            && self.new_api_session == provider.auth.new_api_session
            && self.credential_revision == provider.auth.credential_revision
            && self.check_in_method == provider.automation.check_in_method
            && self.auto_shield == provider.automation.auto_shield
            && self.turnstile_mode == provider.automation.turnstile_mode
    }
}

impl<'a> ProviderService<'a> {
    pub fn new(app: &'a AppHandle) -> Self {
        Self { app }
    }

    pub fn background(app: &'a AppHandle) -> Self {
        Self { app }
    }
}

pub(super) fn find_provider(data: &AppData, id: &str) -> Result<Provider, String> {
    data.providers
        .iter()
        .find(|provider| provider.identity.id == id)
        .cloned()
        .ok_or_else(|| "中转站不存在".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn check_in_policy_changes_invalidate_old_request_contexts() {
        let original = provider("fixture");
        let context = super::ProviderRequestContext::capture(&original);
        let mut edited = original.clone();
        edited.automation.auto_shield = false;
        assert!(!context.matches(&edited));
        edited = original.clone();
        edited.automation.check_in_method = crate::models::ProviderCheckInMethod::SessionSignIn;
        assert!(!context.matches(&edited));
        edited = original;
        edited.automation.turnstile_mode = crate::models::ProviderTurnstileMode::Always;
        assert!(!context.matches(&edited));
    }
    use crate::models::{Provider, ProviderInput};

    fn provider(id: &str) -> Provider {
        Provider::from_input(ProviderInput::default(), id.to_string())
    }

    #[test]
    fn provider_revision_is_not_persisted() {
        let mut provider = provider("provider-1");
        provider.revision = 9;
        let serialized = serde_json::to_value(provider).expect("serialize provider");
        assert!(serialized.get("revision").is_none());
    }

    #[test]
    fn app_revision_is_not_persisted() {
        let data = crate::models::AppData {
            revision: 9,
            ..Default::default()
        };
        let serialized = serde_json::to_value(data).expect("serialize app data");
        assert!(serialized.get("revision").is_none());
    }
}
