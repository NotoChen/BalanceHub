use super::{
    defaults,
    normalize::{
        backup_url_list, normalize_api_key_for_protocol, provider_name_from_input, session_value,
        string_list,
    },
    state::{
        Provider, ProviderAuth, ProviderAutomation, ProviderAutomationInput, ProviderCapabilities,
        ProviderCli, ProviderCliInput, ProviderIdentity, ProviderIdentityInput, ProviderLiveness,
        ProviderLivenessInput, ProviderNotification, ProviderProxy, ProviderQuota, ProviderRuntime,
        ProviderRuntimeInput,
    },
};
use crate::limits;
use crate::models::{
    default_liveness_interval, default_liveness_random_min_interval, default_liveness_timeout,
    AuthMode, AuthSource, LivenessIntervalMode, LivenessPromptMode, ProviderNotificationMode,
    ProviderProtocol, ProviderProxyMode, ProviderQuotaScope, ProviderStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: Option<String>,
    pub identity: ProviderIdentityInput,
    pub auth: ProviderAuth,
    #[serde(default)]
    pub cli: ProviderCliInput,
    pub automation: ProviderAutomationInput,
    pub liveness: ProviderLivenessInput,
    pub proxy: ProviderProxy,
    pub notification: ProviderNotification,
    pub runtime: ProviderRuntimeInput,
}

impl Default for ProviderInput {
    fn default() -> Self {
        Self {
            id: None,
            identity: ProviderIdentityInput {
                name: String::new(),
                base_url: String::new(),
                protocol: ProviderProtocol::default(),
                user_id: String::new(),
                backup_urls: Vec::new(),
            },
            auth: ProviderAuth {
                mode: AuthMode::Password,
                source: AuthSource::Password,
                api_key: String::new(),
                api_key_token_id: String::new(),
                api_key_options: Vec::new(),
                access_token: String::new(),
                session_cookie: String::new(),
                api_user: String::new(),
                login_username: String::new(),
                login_password: String::new(),
                refresh_token: String::new(),
                access_token_expires_at: None,
            },
            cli: ProviderCliInput::default(),
            automation: ProviderAutomationInput {
                refresh_interval: 0,
                check_in_time: String::new(),
            },
            liveness: ProviderLivenessInput {
                use_global: true,
                enabled: false,
                openai_base_url: String::new(),
                anthropic_base_url: String::new(),
                cli_kind: None,
                interval_mode: LivenessIntervalMode::default(),
                interval: default_liveness_interval(),
                random_min_interval: default_liveness_random_min_interval(),
                random_max_interval: default_liveness_interval(),
                timeout: default_liveness_timeout(),
                model: String::new(),
                prompt_mode: LivenessPromptMode::default(),
                fixed_prompt: String::new(),
            },
            proxy: ProviderProxy {
                mode: ProviderProxyMode::default(),
                url: String::new(),
            },
            notification: ProviderNotification {
                mode: ProviderNotificationMode::default(),
                channel_ids: Vec::new(),
            },
            runtime: ProviderRuntimeInput { enabled: true },
        }
    }
}

impl Provider {
    pub fn from_input(input: ProviderInput, id: String) -> Self {
        let name = provider_name_from_input(&input.identity.name, &input.identity.base_url);
        let protocol = input.identity.protocol;
        let api = matches!(protocol, ProviderProtocol::Api);
        let mut auth = input.auth;
        let input_user_id = if !input.identity.user_id.trim().is_empty() {
            input.identity.user_id.trim().to_string()
        } else {
            auth.api_user.trim().to_string()
        };
        if api {
            auth.mode = AuthMode::ApiKey;
        }
        Self {
            identity: ProviderIdentity {
                id,
                name,
                base_url: input.identity.base_url,
                protocol,
                display_name: String::new(),
                username: String::new(),
                user_id: if matches!(auth.mode, AuthMode::ApiKey) {
                    String::new()
                } else {
                    input_user_id
                },
                site_logo: String::new(),
                backup_urls: backup_url_list(input.identity.backup_urls),
            },
            auth: normalize_provider_auth(auth, protocol),
            quota: ProviderQuota {
                available: 0.0,
                used: 0.0,
                known: !api,
                total_known: !api,
                scope: if api {
                    ProviderQuotaScope::Token
                } else {
                    ProviderQuotaScope::Account
                },
                unlimited: false,
                per_unit: defaults::quota_per_unit(),
                display_type: defaults::quota_display_type(),
                currency_symbol: defaults::currency_symbol(),
                currency_exchange_rate: defaults::currency_exchange_rate(),
            },
            capabilities: ProviderCapabilities::default(),
            cli: ProviderCli {
                preferred_model: input.cli.preferred_model.trim().to_string(),
            },
            automation: ProviderAutomation {
                refresh_interval: input.automation.refresh_interval,
                check_in_time: input.automation.check_in_time,
                last_synced_at: None,
                last_checked_in_at: None,
                last_check_in_user: String::new(),
                check_in_records: Vec::new(),
            },
            liveness: ProviderLiveness {
                use_global: input.liveness.use_global,
                enabled: input.liveness.enabled,
                openai_base_url: input.liveness.openai_base_url,
                anthropic_base_url: input.liveness.anthropic_base_url,
                cli_kind: input.liveness.cli_kind,
                interval_mode: input.liveness.interval_mode,
                interval: input.liveness.interval,
                random_min_interval: input.liveness.random_min_interval,
                random_max_interval: input.liveness.random_max_interval,
                timeout: input
                    .liveness
                    .timeout
                    .clamp(10, limits::MAX_LIVENESS_TIMEOUT_SECS),
                model: input.liveness.model,
                prompt_mode: input.liveness.prompt_mode,
                fixed_prompt: input.liveness.fixed_prompt,
                prompt_cursor: 0,
                next_at: None,
                records: Vec::new(),
                run_count: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_tokens: 0,
                total_cost_usd: 0.0,
            },
            proxy: input.proxy,
            notification: ProviderNotification {
                mode: input.notification.mode,
                channel_ids: string_list(input.notification.channel_ids),
            },
            runtime: ProviderRuntime {
                enabled: input.runtime.enabled,
                status: ProviderStatus::Warning,
                error_message: Some("尚未同步".to_string()),
            },
        }
    }

    /// Append a manually supplied API Key to an existing provider card without
    /// replacing its account credentials or display state.
    pub fn add_api_key(&mut self, raw_key: &str) -> Result<(), String> {
        let key = normalize_api_key_for_protocol(raw_key, self.identity.protocol);
        if key.is_empty() {
            return Err("API Key 为空，无法添加".to_string());
        }
        if self.auth.api_key_options.iter().any(|option| {
            normalize_api_key_for_protocol(&option.key, self.identity.protocol) == key
        }) || normalize_api_key_for_protocol(&self.auth.api_key, self.identity.protocol) == key
        {
            return Err("该 API Key 已存在".to_string());
        }

        let mut option =
            crate::models::ProviderApiKeyOption::current_for_protocol(&key, self.identity.protocol);
        option.name = "手动添加 API Key".to_string();
        self.auth.api_key_options.insert(0, option);
        if self.auth.api_key.trim().is_empty() {
            self.auth.api_key = key;
            self.auth.api_key_token_id.clear();
        }
        self.auth = normalize_provider_auth(self.auth.clone(), self.identity.protocol);
        Ok(())
    }

    pub fn apply_input(&mut self, input: ProviderInput) {
        let previous_check_in_user = self.auth.api_user.trim();
        let protocol_changed = self.identity.protocol != input.identity.protocol;
        let next_auth_mode = if matches!(input.identity.protocol, ProviderProtocol::Api) {
            AuthMode::ApiKey
        } else {
            input.auth.mode
        };
        let password_session_invalidated = matches!(self.auth.mode, AuthMode::Password)
            && matches!(next_auth_mode, AuthMode::Password)
            && (self.auth.login_username != input.auth.login_username
                || self.auth.login_password != input.auth.login_password
                || self.identity.base_url.trim_end_matches('/')
                    != input.identity.base_url.trim_end_matches('/')
                || protocol_changed);
        let next_session_cookie = if password_session_invalidated || protocol_changed {
            String::new()
        } else {
            input.auth.session_cookie.clone()
        };
        let next_api_user = if password_session_invalidated || protocol_changed {
            String::new()
        } else {
            input.auth.api_user.clone()
        };
        let same_access_token = self.auth.access_token == input.auth.access_token;
        let next_access_token =
            if password_session_invalidated || (protocol_changed && same_access_token) {
                String::new()
            } else {
                input.auth.access_token.clone()
            };
        // refresh_token / 过期时刻与 access_token 成对：token 清空时一并清空；access_token
        // 未被用户改动时保留后端管理的刷新令牌，避免前端回传缺字段把它抹掉。
        let next_refresh_token = if next_access_token.is_empty() {
            String::new()
        } else if same_access_token {
            self.auth.refresh_token.clone()
        } else {
            input.auth.refresh_token.clone()
        };
        let next_access_token_expires_at = if next_access_token.is_empty() {
            None
        } else if same_access_token {
            self.auth.access_token_expires_at
        } else {
            input.auth.access_token_expires_at
        };
        let next_check_in_user = next_api_user.trim();
        let session_changed = previous_check_in_user.is_empty()
            && next_check_in_user.is_empty()
            && session_value(&self.auth.session_cookie) != session_value(&next_session_cookie);
        if protocol_changed || previous_check_in_user != next_check_in_user || session_changed {
            self.automation.last_checked_in_at = None;
            self.automation.last_check_in_user = String::new();
            self.automation.check_in_records.clear();
        }

        let input_user_id = if !input.identity.user_id.trim().is_empty() {
            input.identity.user_id.trim().to_string()
        } else {
            input.auth.api_user.trim().to_string()
        };
        let identity_user_id = if protocol_changed
            || matches!(next_auth_mode, AuthMode::ApiKey)
            || previous_check_in_user != next_check_in_user
            || session_changed
        {
            String::new()
        } else {
            input_user_id
        };

        self.identity.name =
            provider_name_from_input(&input.identity.name, &input.identity.base_url);
        self.identity.base_url = input.identity.base_url;
        self.identity.protocol = input.identity.protocol;
        self.identity.user_id = identity_user_id;
        self.identity.backup_urls = backup_url_list(input.identity.backup_urls);
        if protocol_changed {
            self.identity.display_name.clear();
            self.identity.username.clear();
            self.identity.user_id.clear();
            self.identity.site_logo.clear();
            self.quota.available = 0.0;
            self.quota.used = 0.0;
            let account_protocol = !matches!(self.identity.protocol, ProviderProtocol::Api);
            self.quota.known = account_protocol;
            self.quota.total_known = account_protocol;
            self.quota.unlimited = false;
            self.quota.scope = if account_protocol {
                ProviderQuotaScope::Account
            } else {
                ProviderQuotaScope::Token
            };
            self.capabilities = ProviderCapabilities::default();
            self.automation.last_synced_at = None;
            self.runtime.status = ProviderStatus::Warning;
            self.runtime.error_message = Some("尚未同步".to_string());
        }
        let next_api_key_token_id = if protocol_changed {
            String::new()
        } else {
            input.auth.api_key_token_id
        };
        let next_api_key_options = if protocol_changed {
            Vec::new()
        } else {
            input.auth.api_key_options
        };
        self.auth = normalize_provider_auth(
            ProviderAuth {
                mode: next_auth_mode,
                source: input.auth.source,
                api_key: input.auth.api_key,
                api_key_token_id: next_api_key_token_id,
                api_key_options: next_api_key_options,
                access_token: next_access_token,
                session_cookie: next_session_cookie,
                api_user: next_api_user,
                login_username: input.auth.login_username,
                login_password: input.auth.login_password,
                refresh_token: next_refresh_token,
                access_token_expires_at: next_access_token_expires_at,
            },
            self.identity.protocol,
        );
        self.cli.preferred_model = input.cli.preferred_model.trim().to_string();
        self.automation.refresh_interval = input.automation.refresh_interval;
        self.automation.check_in_time = input.automation.check_in_time;
        self.proxy = input.proxy;
        self.notification.mode = input.notification.mode;
        self.notification.channel_ids = string_list(input.notification.channel_ids);
        self.liveness.use_global = input.liveness.use_global;
        self.liveness.enabled = input.liveness.enabled;
        self.liveness.openai_base_url = input.liveness.openai_base_url;
        self.liveness.anthropic_base_url = input.liveness.anthropic_base_url;
        self.liveness.cli_kind = input.liveness.cli_kind;
        self.liveness.interval_mode = input.liveness.interval_mode;
        self.liveness.interval = input.liveness.interval;
        self.liveness.random_min_interval = input.liveness.random_min_interval;
        self.liveness.random_max_interval = input.liveness.random_max_interval;
        self.liveness.timeout = input
            .liveness
            .timeout
            .clamp(10, limits::MAX_LIVENESS_TIMEOUT_SECS);
        self.liveness.model = input.liveness.model;
        self.liveness.prompt_mode = input.liveness.prompt_mode;
        self.liveness.fixed_prompt = input.liveness.fixed_prompt;
        self.runtime.enabled = input.runtime.enabled;
    }
}

pub(crate) fn normalize_provider_auth(
    mut auth: ProviderAuth,
    protocol: ProviderProtocol,
) -> ProviderAuth {
    // Sub2API 没有会话 Cookie 概念；防御性纠正，避免非法组合被持久化（导入等旁路）。
    if matches!(protocol, ProviderProtocol::Sub2Api) && matches!(auth.mode, AuthMode::Session) {
        auth.mode = AuthMode::Password;
    }
    // 来源(source)由凭据种类(mode)推导，mode 为权威——避免来源的默认值反向覆盖显式选择的
    // 凭据。OAuth 是 Cookie/令牌的一种来源，与手动并列，故在 Session/AccessToken 下保留既有
    // OAuth 标记；账号密码本身就是一种来源；裸 API Key 永远是手动。
    auth.source = match auth.mode {
        AuthMode::Password => AuthSource::Password,
        AuthMode::ApiKey => AuthSource::Manual,
        AuthMode::Session | AuthMode::AccessToken => {
            if matches!(auth.source, AuthSource::Oauth) {
                AuthSource::Oauth
            } else {
                AuthSource::Manual
            }
        }
    };
    auth.api_key = normalize_api_key_for_protocol(&auth.api_key, protocol);
    auth.api_key_token_id = auth.api_key_token_id.trim().to_string();

    let mut options = Vec::new();
    for option in auth.api_key_options {
        let option = option.normalize_for_protocol(protocol);
        let duplicate = options
            .iter()
            .any(|known: &crate::models::ProviderApiKeyOption| {
                (!option.token_id.is_empty() && option.token_id == known.token_id)
                    || (!option.key.is_empty() && option.key == known.key)
            });
        if !duplicate {
            options.push(option);
        }
    }

    if !auth.api_key.is_empty() {
        // A list refresh can retain token metadata while the remote endpoint
        // refuses to reveal the full value. Re-associate the locally selected
        // key by token id before deciding whether a synthetic entry is needed.
        if !auth.api_key_token_id.is_empty() {
            if let Some(option) = options
                .iter_mut()
                .find(|option| option.token_id == auth.api_key_token_id)
            {
                if !option.key_available {
                    option.key = auth.api_key.clone();
                    option.key_available = true;
                    if option.masked_key.is_empty() {
                        option.masked_key =
                            crate::models::ProviderApiKeyOption::current_for_protocol(
                                &auth.api_key,
                                protocol,
                            )
                            .masked_key;
                    }
                }
            }
        }
        if !options.iter().any(|option| option.key == auth.api_key) {
            let mut current =
                crate::models::ProviderApiKeyOption::current_for_protocol(&auth.api_key, protocol);
            current.token_id = auth.api_key_token_id.clone();
            options.insert(0, current);
        }
    }

    if auth.api_key_token_id.is_empty() {
        if let Some(option) = options.iter().find(|option| option.key == auth.api_key) {
            auth.api_key_token_id = option.token_id.clone();
        }
    }
    auth.api_key_options = options;
    auth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_provider_uses_key_scope_without_rewriting_the_key() {
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.auth.mode = AuthMode::Password;
        input.auth.api_key = "gsk_custom-key".to_string();

        let provider = Provider::from_input(input, "generic-test".to_string());

        assert_eq!(provider.auth.mode, AuthMode::ApiKey);
        assert_eq!(provider.auth.api_key, "gsk_custom-key");
        assert_eq!(provider.quota.scope, ProviderQuotaScope::Token);
        assert!(!provider.quota.known);
        assert!(!provider.quota.total_known);
    }

    #[test]
    fn adding_api_key_keeps_the_existing_account_card_and_deduplicates_keys() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://relay.example.com".to_string();
        input.auth.login_username = "alice".to_string();
        input.auth.login_password = "password".to_string();
        let mut provider = Provider::from_input(input, "provider-test".to_string());

        provider
            .add_api_key("sk-extra")
            .expect("key should be added");
        assert_eq!(provider.auth.mode, AuthMode::Password);
        assert_eq!(provider.auth.login_username, "alice");
        assert_eq!(provider.auth.api_key_options.len(), 1);
        assert_eq!(provider.auth.api_key_options[0].key, "sk-extra");
        assert!(provider.add_api_key("sk-extra").is_err());
    }

    #[test]
    fn switching_protocol_clears_stale_derived_state() {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        provider.identity.display_name = "Account".to_string();
        provider.identity.username = "alice".to_string();
        provider.identity.user_id = "42".to_string();
        provider.quota.available = 100.0;
        provider.capabilities.available_models = vec!["old-model".to_string()];
        provider.automation.last_synced_at = Some("123".to_string());
        provider.automation.last_checked_in_at = Some("456".to_string());
        provider.automation.last_check_in_user = "old-user".to_string();
        provider
            .automation
            .check_in_records
            .push(crate::models::ProviderCheckInRecord {
                date: "2026-07-28".to_string(),
                checked_at: Some("456".to_string()),
                quota_delta: Some(1.0),
                message: "签到成功".to_string(),
            });
        provider.auth.api_key_token_id = "11".to_string();
        provider.auth.api_key_options =
            vec![crate::models::ProviderApiKeyOption::current("sk-old-key")];

        let input = ProviderInput {
            id: Some(provider.identity.id.clone()),
            identity: ProviderIdentityInput {
                protocol: ProviderProtocol::Api,
                ..ProviderIdentityInput::default()
            },
            auth: ProviderAuth {
                mode: AuthMode::ApiKey,
                api_key: "key_without_sk_prefix".to_string(),
                ..ProviderInput::default().auth
            },
            ..ProviderInput::default()
        };
        provider.apply_input(input);

        assert_eq!(provider.identity.protocol, ProviderProtocol::Api);
        assert_eq!(provider.auth.api_key, "key_without_sk_prefix");
        assert!(provider.identity.display_name.is_empty());
        assert!(provider.identity.username.is_empty());
        assert!(provider.identity.user_id.is_empty());
        assert!(provider.automation.last_synced_at.is_none());
        assert!(provider.automation.last_checked_in_at.is_none());
        assert!(provider.automation.last_check_in_user.is_empty());
        assert!(provider.automation.check_in_records.is_empty());
        assert!(provider.capabilities.available_models.is_empty());
        assert!(provider.auth.api_key_token_id.is_empty());
        assert_eq!(provider.auth.api_key_options.len(), 1);
        assert!(provider.auth.api_key_options[0].token_id.is_empty());
        assert!(!provider.quota.known);
    }

    #[test]
    fn switching_between_account_protocols_clears_old_account_state() {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        provider.identity.username = "newapi-user".to_string();
        provider.identity.site_logo = "https://example.com/logo.png".to_string();
        provider.quota.available = 100.0;
        provider.capabilities.api_key_management_known = true;
        provider.capabilities.available_models = vec!["old-model".to_string()];
        provider.automation.last_checked_in_at = Some("123".to_string());

        let mut input = ProviderInput {
            id: Some(provider.identity.id.clone()),
            ..ProviderInput::default()
        };
        input.identity.protocol = ProviderProtocol::Sub2Api;
        input.auth.mode = AuthMode::Password;
        input.auth.login_username = "sub2@example.com".to_string();
        input.auth.login_password = "password".to_string();
        provider.apply_input(input);

        assert_eq!(provider.identity.protocol, ProviderProtocol::Sub2Api);
        assert!(provider.identity.username.is_empty());
        assert!(provider.identity.site_logo.is_empty());
        assert_eq!(provider.quota.available, 0.0);
        assert_eq!(provider.quota.scope, ProviderQuotaScope::Account);
        assert!(provider.capabilities.available_models.is_empty());
        assert!(!provider.capabilities.api_key_management_known);
        assert!(provider.automation.last_checked_in_at.is_none());
    }
}
