use super::{
    defaults,
    normalize::{
        backup_url_list, normalize_api_key, provider_name_from_input, session_value, string_list,
    },
    state::{
        Provider, ProviderAuth, ProviderAutomation, ProviderAutomationInput, ProviderCapabilities,
        ProviderCli, ProviderCliInput, ProviderIdentity, ProviderIdentityInput, ProviderLiveness,
        ProviderLivenessInput, ProviderNotification, ProviderProxy, ProviderQuota, ProviderRuntime,
        ProviderRuntimeInput,
    },
};
use crate::models::{
    default_liveness_interval, default_liveness_random_min_interval, default_liveness_timeout,
    AuthMode, LivenessIntervalMode, LivenessPromptMode, ProviderNotificationMode,
    ProviderProxyMode, ProviderQuotaScope, ProviderStatus,
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
                backup_urls: Vec::new(),
            },
            auth: ProviderAuth {
                mode: AuthMode::Password,
                api_key: String::new(),
                api_key_token_id: String::new(),
                api_key_options: Vec::new(),
                access_token: String::new(),
                session_cookie: String::new(),
                api_user: String::new(),
                login_username: String::new(),
                login_password: String::new(),
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
        Self {
            identity: ProviderIdentity {
                id,
                name,
                base_url: input.identity.base_url,
                display_name: String::new(),
                username: String::new(),
                user_id: String::new(),
                site_logo: String::new(),
                backup_urls: backup_url_list(input.identity.backup_urls),
            },
            auth: normalize_provider_auth(input.auth),
            quota: ProviderQuota {
                available: 0.0,
                used: 0.0,
                scope: ProviderQuotaScope::Account,
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
                timeout: input.liveness.timeout,
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

    pub fn apply_input(&mut self, input: ProviderInput) {
        let previous_check_in_user = self.auth.api_user.trim();
        let password_session_invalidated = matches!(self.auth.mode, AuthMode::Password)
            && matches!(input.auth.mode, AuthMode::Password)
            && (self.auth.login_username != input.auth.login_username
                || self.auth.login_password != input.auth.login_password
                || self.identity.base_url.trim_end_matches('/')
                    != input.identity.base_url.trim_end_matches('/'));
        let next_session_cookie = if password_session_invalidated {
            String::new()
        } else {
            input.auth.session_cookie.clone()
        };
        let next_api_user = if password_session_invalidated {
            String::new()
        } else {
            input.auth.api_user.clone()
        };
        let next_check_in_user = next_api_user.trim();
        let session_changed = previous_check_in_user.is_empty()
            && next_check_in_user.is_empty()
            && session_value(&self.auth.session_cookie) != session_value(&next_session_cookie);
        if previous_check_in_user != next_check_in_user || session_changed {
            self.automation.last_checked_in_at = None;
            self.automation.last_check_in_user = String::new();
            self.automation.check_in_records.clear();
        }

        self.identity.name =
            provider_name_from_input(&input.identity.name, &input.identity.base_url);
        self.identity.base_url = input.identity.base_url;
        self.identity.backup_urls = backup_url_list(input.identity.backup_urls);
        self.auth = normalize_provider_auth(ProviderAuth {
            mode: input.auth.mode,
            api_key: input.auth.api_key,
            api_key_token_id: input.auth.api_key_token_id,
            api_key_options: input.auth.api_key_options,
            access_token: input.auth.access_token,
            session_cookie: next_session_cookie,
            api_user: next_api_user,
            login_username: input.auth.login_username,
            login_password: input.auth.login_password,
        });
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
        self.liveness.timeout = input.liveness.timeout;
        self.liveness.model = input.liveness.model;
        self.liveness.prompt_mode = input.liveness.prompt_mode;
        self.liveness.fixed_prompt = input.liveness.fixed_prompt;
        self.runtime.enabled = input.runtime.enabled;
    }
}

pub(crate) fn normalize_provider_auth(mut auth: ProviderAuth) -> ProviderAuth {
    auth.api_key = normalize_api_key(&auth.api_key);
    auth.api_key_token_id = auth.api_key_token_id.trim().to_string();

    let mut options = Vec::new();
    for option in auth.api_key_options {
        let option = option.normalize();
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
                            crate::models::ProviderApiKeyOption::current(&auth.api_key).masked_key;
                    }
                }
            }
        }
        if !options.iter().any(|option| option.key == auth.api_key) {
            let mut current = crate::models::ProviderApiKeyOption::current(&auth.api_key);
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
