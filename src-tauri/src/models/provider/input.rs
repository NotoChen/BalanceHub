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
    is_full_api_key_value, AgentCliKind, AuthMode, AuthSource, LivenessIntervalMode,
    LivenessPromptMode, ProviderNotificationMode, ProviderProtocol, ProviderProxyMode,
    ProviderQuotaScope, ProviderStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
                remark: String::new(),
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
                agent_base_urls: BTreeMap::new(),
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
            revision: 0,
            identity: ProviderIdentity {
                id,
                name,
                base_url: input.identity.base_url,
                protocol,
                remark: limits::normalize_provider_remark(&input.identity.remark),
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
                agent_base_urls: normalize_agent_base_urls(input.liveness.agent_base_urls),
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
        self.add_named_api_key(raw_key, "")
    }

    pub fn add_named_api_key(&mut self, raw_key: &str, remark: &str) -> Result<(), String> {
        let key = normalize_api_key_for_protocol(raw_key, self.identity.protocol);
        if !is_full_api_key_value(&key) {
            return Err(if key.contains('*') {
                "不能添加脱敏 API Key，请填写完整值".to_string()
            } else {
                "API Key 为空，无法添加".to_string()
            });
        }
        if self.auth.api_key_options.len() >= limits::MAX_API_KEYS_PER_PROVIDER {
            return Err(format!(
                "API Key 数量已达到上限（{} 个）",
                limits::MAX_API_KEYS_PER_PROVIDER
            ));
        }
        if self.auth.api_key_options.iter().any(|option| {
            normalize_api_key_for_protocol(&option.key, self.identity.protocol) == key
        }) || normalize_api_key_for_protocol(&self.auth.api_key, self.identity.protocol) == key
        {
            return Err("该 API Key 已存在".to_string());
        }

        let mut option =
            crate::models::ProviderApiKeyOption::current_for_protocol(&key, self.identity.protocol);
        option.name.clear();
        option.local_name = limits::normalize_api_key_remark(remark);
        self.auth.api_key_options.insert(0, option);
        if self.auth.api_key.trim().is_empty() {
            self.auth.api_key = key;
            self.auth.api_key_token_id.clear();
        }
        self.auth = normalize_provider_auth(self.auth.clone(), self.identity.protocol);
        Ok(())
    }

    pub fn set_api_key_remark(&mut self, local_id: &str, remark: &str) -> Result<bool, String> {
        let local_id = local_id.trim();
        if local_id.is_empty() {
            return Err("缺少 API Key 标识".to_string());
        }
        let normalized_remark = limits::normalize_api_key_remark(remark);
        let option = self
            .auth
            .api_key_options
            .iter_mut()
            .find(|option| option.local_id == local_id)
            .ok_or_else(|| "API Key 已不存在，请刷新后重试".to_string())?;
        if option.local_name == normalized_remark {
            return Ok(false);
        }
        option.local_name = normalized_remark;
        self.auth = normalize_provider_auth(self.auth.clone(), self.identity.protocol);
        Ok(true)
    }

    pub fn set_default_api_key(&mut self, local_id: &str) -> Result<(), String> {
        let local_id = local_id.trim();
        let option = self
            .auth
            .api_key_options
            .iter()
            .find(|option| option.local_id == local_id)
            .cloned()
            .ok_or_else(|| "API Key 已不存在，请刷新后重试".to_string())?;
        if !option.key_available || !is_full_api_key_value(&option.key) {
            return Err("该 API Key 未读取到完整值，无法设为当前调用 Key".to_string());
        }
        self.auth.api_key = option.key;
        self.auth.api_key_token_id = option.token_id;
        self.capabilities.available_models.clear();
        self.automation.last_synced_at = None;
        self.auth = normalize_provider_auth(self.auth.clone(), self.identity.protocol);
        Ok(())
    }

    pub fn remove_local_api_key(&mut self, local_id: &str) -> Result<(), String> {
        let local_id = local_id.trim();
        let index = self
            .auth
            .api_key_options
            .iter()
            .position(|option| option.local_id == local_id)
            .ok_or_else(|| "API Key 已不存在，请刷新后重试".to_string())?;
        let removed = self.auth.api_key_options.remove(index);
        let removed_default =
            normalize_api_key_for_protocol(&self.auth.api_key, self.identity.protocol)
                == removed.key;
        if removed_default {
            if let Some(next) = self
                .auth
                .api_key_options
                .iter()
                .find(|option| option.key_available && is_full_api_key_value(&option.key))
            {
                self.auth.api_key = next.key.clone();
                self.auth.api_key_token_id = next.token_id.clone();
            } else {
                self.auth.api_key.clear();
                self.auth.api_key_token_id.clear();
            }
            self.capabilities.available_models.clear();
            self.automation.last_synced_at = None;
        }
        self.auth = normalize_provider_auth(self.auth.clone(), self.identity.protocol);
        Ok(())
    }

    pub fn apply_input(&mut self, input: ProviderInput) {
        let previous_check_in_user = self.auth.api_user.trim();
        let protocol_changed = self.identity.protocol != input.identity.protocol;
        let base_url_changed = self.identity.base_url.trim_end_matches('/')
            != input.identity.base_url.trim_end_matches('/');
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
        let auth_material_changed = protocol_changed
            || base_url_changed
            || self.auth.mode != next_auth_mode
            || self.auth.api_key != input.auth.api_key
            || self.auth.access_token != next_access_token
            || self.auth.session_cookie != next_session_cookie
            || self.auth.api_user != next_api_user
            || self.auth.login_username != input.auth.login_username
            || self.auth.login_password != input.auth.login_password
            || self.auth.refresh_token != next_refresh_token
            || self.auth.access_token_expires_at != next_access_token_expires_at;
        let next_check_in_user = next_api_user.trim();
        let session_changed = previous_check_in_user.is_empty()
            && next_check_in_user.is_empty()
            && session_value(&self.auth.session_cookie) != session_value(&next_session_cookie);
        if auth_material_changed || previous_check_in_user != next_check_in_user || session_changed
        {
            self.automation.last_checked_in_at = None;
            self.automation.last_check_in_user = String::new();
            self.automation.check_in_records.clear();
        }

        let input_user_id = if !input.identity.user_id.trim().is_empty() {
            input.identity.user_id.trim().to_string()
        } else {
            input.auth.api_user.trim().to_string()
        };
        let identity_user_id = if auth_material_changed
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
        self.identity.remark = limits::normalize_provider_remark(&input.identity.remark);
        self.identity.user_id = identity_user_id;
        self.identity.backup_urls = backup_url_list(input.identity.backup_urls);
        if auth_material_changed {
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
        self.liveness.agent_base_urls = normalize_agent_base_urls(input.liveness.agent_base_urls);
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

    let mut options: Vec<crate::models::ProviderApiKeyOption> = Vec::new();
    for option in auth.api_key_options {
        let option = option.normalize_for_protocol(protocol);
        if let Some(known) = options.iter_mut().find(|known| {
            (!option.local_id.is_empty() && option.local_id == known.local_id)
                || (!option.token_id.is_empty() && option.token_id == known.token_id)
                || (!option.key.is_empty() && option.key == known.key)
        }) {
            // A duplicate remote snapshot must not erase a local remark that
            // was attached to the same stable key identity.
            if known.local_name.is_empty() && !option.local_name.is_empty() {
                known.local_name = option.local_name;
            }
            if known.name.is_empty() && !option.name.is_empty() {
                known.name = option.name;
            }
        } else {
            options.push(option);
        }
    }

    if !auth.api_key.is_empty() {
        if let Some(option) = options.iter().find(|option| option.key == auth.api_key) {
            // The full configured key is the credential that requests really
            // use. Repair stale remote metadata from that value, not the other
            // way around.
            auth.api_key_token_id = option.token_id.clone();
        } else {
            // A list refresh can retain token metadata while the remote endpoint
            // refuses to reveal the full value. Token ID is only a fallback for
            // such a redacted entry; it must never replace a different full key.
            let restored_redacted_option = if auth.api_key_token_id.is_empty() {
                false
            } else if let Some(option) = options
                .iter_mut()
                .find(|option| option.token_id == auth.api_key_token_id && !option.key_available)
            {
                option.key = auth.api_key.clone();
                option.key_available = true;
                if option.masked_key.is_empty() {
                    option.masked_key = crate::models::ProviderApiKeyOption::current_for_protocol(
                        &auth.api_key,
                        protocol,
                    )
                    .masked_key;
                }
                true
            } else {
                false
            };

            if !restored_redacted_option {
                let token_points_to_another_key = !auth.api_key_token_id.is_empty()
                    && options.iter().any(|option| {
                        option.token_id == auth.api_key_token_id
                            && option.key_available
                            && option.key != auth.api_key
                    });
                if token_points_to_another_key {
                    auth.api_key_token_id.clear();
                }
            }

            if !options.iter().any(|option| option.key == auth.api_key) {
                let mut current = crate::models::ProviderApiKeyOption::current_for_protocol(
                    &auth.api_key,
                    protocol,
                );
                current.token_id = auth.api_key_token_id.clone();
                options.insert(0, current);
            }
        }
    }

    if auth.api_key_token_id.is_empty() && !auth.api_key.is_empty() {
        if let Some(option) = options.iter().find(|option| option.key == auth.api_key) {
            auth.api_key_token_id = option.token_id.clone();
        }
    }
    auth.api_key_options = options;
    auth
}

fn normalize_agent_base_urls(
    values: BTreeMap<AgentCliKind, String>,
) -> BTreeMap<AgentCliKind, String> {
    values
        .into_iter()
        .filter_map(|(kind, value)| {
            let value = value.trim().to_string();
            (!value.is_empty()).then_some((kind, value))
        })
        .collect()
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
    fn provider_remark_is_saved_and_editable_without_invalidating_synced_state() {
        let mut input = ProviderInput::default();
        input.identity.remark = "  主用\n中转站  ".to_string();
        let mut provider = Provider::from_input(input.clone(), "provider-test".to_string());
        provider.capabilities.available_models = vec!["model-a".to_string()];
        provider.automation.last_synced_at = Some("123".to_string());

        assert_eq!(provider.identity.remark, "主用中转站");

        input.id = Some(provider.identity.id.clone());
        input.identity.remark = "  备用站  ".to_string();
        provider.apply_input(input);

        assert_eq!(provider.identity.remark, "备用站");
        assert_eq!(provider.capabilities.available_models, ["model-a"]);
        assert_eq!(provider.automation.last_synced_at.as_deref(), Some("123"));
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
        assert!(provider.auth.api_key_options[0].name.is_empty());
        assert!(provider.auth.api_key_options[0].local_name.is_empty());
        assert!(!provider.auth.api_key_options[0].local_id.is_empty());
        assert!(provider.add_api_key("sk-extra").is_err());
    }

    #[test]
    fn auth_normalization_prefers_the_configured_key_over_a_stale_token_id() {
        let mut auth = ProviderInput::default().auth;
        auth.api_key = "sk-first".to_string();
        auth.api_key_token_id = "token-second".to_string();
        let mut first = crate::models::ProviderApiKeyOption::current("sk-first");
        first.token_id = "token-first".to_string();
        let mut second = crate::models::ProviderApiKeyOption::current("sk-second");
        second.token_id = "token-second".to_string();
        auth.api_key_options = vec![first, second];

        let normalized = normalize_provider_auth(auth, ProviderProtocol::NewApi);

        assert_eq!(normalized.api_key, "sk-first");
        assert_eq!(normalized.api_key_token_id, "token-first");
        assert_eq!(normalized.api_key_options.len(), 2);
    }

    #[test]
    fn local_key_management_keeps_default_selection_and_removes_keys() {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        provider
            .add_named_api_key("sk-first", "第一把")
            .expect("first key");
        provider
            .add_named_api_key("sk-second", "第二把")
            .expect("second key");
        let first_id = provider
            .auth
            .api_key_options
            .iter()
            .find(|option| option.key == "sk-first")
            .expect("first option")
            .local_id
            .clone();
        let second_id = provider
            .auth
            .api_key_options
            .iter()
            .find(|option| option.key == "sk-second")
            .expect("second option")
            .local_id
            .clone();

        provider
            .set_default_api_key(&first_id)
            .expect("default should change");
        assert_eq!(provider.auth.api_key, "sk-first");
        assert!(provider
            .set_api_key_remark(&second_id, "  备用\nKey  ")
            .expect("remark should change"));
        assert_eq!(
            provider
                .auth
                .api_key_options
                .iter()
                .find(|option| option.local_id == second_id)
                .expect("remarked option")
                .local_name,
            "备用Key"
        );
        assert_eq!(
            provider
                .auth
                .api_key_options
                .iter()
                .find(|option| option.local_id == first_id)
                .expect("first option")
                .local_name,
            "第一把"
        );
        assert!(!provider
            .set_api_key_remark(&second_id, "备用Key")
            .expect("equal normalized remark should be unchanged"));
        assert!(provider
            .set_api_key_remark(&second_id, "")
            .expect("remark should clear"));
        assert!(!provider
            .set_api_key_remark(&second_id, "  ")
            .expect("equal empty remark should be unchanged"));
        assert!(provider
            .auth
            .api_key_options
            .iter()
            .find(|option| option.local_id == second_id)
            .expect("cleared option")
            .local_name
            .is_empty());
        provider
            .remove_local_api_key(&first_id)
            .expect("local key should remove");
        assert_eq!(provider.auth.api_key, "sk-second");
        assert!(provider
            .auth
            .api_key_options
            .iter()
            .all(|option| option.local_id != first_id));
    }

    #[test]
    fn local_key_removal_can_forget_a_remote_key_without_revoking_it() {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        let mut option = crate::models::ProviderApiKeyOption::current("sk-remote");
        option.token_id = "remote-1".to_string();
        provider.auth.api_key_options = vec![option.normalize()];
        let local_id = provider.auth.api_key_options[0].local_id.clone();

        provider
            .remove_local_api_key(&local_id)
            .expect("remote key can be removed from the local vault");

        assert!(provider.auth.api_key_options.is_empty());
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

    #[test]
    fn changing_credentials_invalidates_capability_cache() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://relay.example.com".to_string();
        input.auth.login_username = "alice".to_string();
        input.auth.login_password = "old-password".to_string();
        let mut provider = Provider::from_input(input.clone(), "provider-test".to_string());
        provider.capabilities.available_models = vec!["cached-model".to_string()];
        provider.capabilities.probed_at = Some("123".to_string());

        input.auth.login_password = "new-password".to_string();
        provider.apply_input(input);

        assert!(provider.capabilities.available_models.is_empty());
        assert_eq!(provider.capabilities.probed_at, None);
        assert!(matches!(provider.runtime.status, ProviderStatus::Warning));
    }

    #[test]
    fn changing_endpoint_invalidates_capability_cache() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://relay.example.com".to_string();
        let mut provider = Provider::from_input(input.clone(), "provider-test".to_string());
        provider.capabilities.available_models = vec!["cached-model".to_string()];

        input.identity.base_url = "https://relay-two.example.com".to_string();
        provider.apply_input(input);

        assert!(provider.capabilities.available_models.is_empty());
    }
}
