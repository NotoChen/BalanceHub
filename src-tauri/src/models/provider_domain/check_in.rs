use crate::models::{AuthMode, Provider, ProviderCheckInMethod, ProviderProtocol};
use serde::Serialize;

use super::auth;

/// Presets are hints for automatic selection. User-selected methods always win.
pub fn preset_for_url(base_url: &str) -> Option<ProviderCheckInMethod> {
    let url = reqwest::Url::parse(base_url.trim()).ok()?;
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if host == "anyrouter.top" || host.ends_with(".anyrouter.top") {
        Some(ProviderCheckInMethod::SessionSignIn)
    } else if host == "agentrouter.org" || host.ends_with(".agentrouter.org") {
        Some(ProviderCheckInMethod::FreshLogin)
    } else {
        None
    }
}

pub fn effective_method(provider: &Provider) -> ProviderCheckInMethod {
    match provider.automation.check_in_method {
        ProviderCheckInMethod::Auto => {
            preset_for_url(&provider.identity.base_url).unwrap_or(ProviderCheckInMethod::Standard)
        }
        method => method,
    }
}

pub fn validate_credentials(provider: &Provider) -> Result<(), String> {
    if provider.identity.protocol != ProviderProtocol::NewApi {
        return Err("当前协议不支持账号签到".to_string());
    }
    if provider.auth.mode == AuthMode::ApiKey {
        return Err("API Key 不支持账号签到，请切换账号认证方式".to_string());
    }
    let has_password = provider.auth.mode == AuthMode::Password
        && !provider.auth.login_username.trim().is_empty()
        && !provider.auth.login_password.trim().is_empty();
    match effective_method(provider) {
        ProviderCheckInMethod::FreshLogin if !has_password => {
            Err("重新登录签到需要选择账号密码认证，并填写用户名和密码".to_string())
        }
        ProviderCheckInMethod::SessionSignIn if !auth::has_session(provider) && !has_password => {
            Err("会话签到需要 session Cookie，或使用账号密码取得会话".to_string())
        }
        ProviderCheckInMethod::Auto | ProviderCheckInMethod::Standard => {
            let authenticated = match provider.auth.mode {
                AuthMode::Session | AuthMode::Password => {
                    auth::has_session(provider) && auth::has_api_user(provider)
                }
                AuthMode::AccessToken => {
                    auth::has_access_token(provider) && auth::has_api_user(provider)
                }
                AuthMode::ApiKey => false,
            };
            if authenticated || has_password {
                Ok(())
            } else {
                Err("标准签到需要账号密码，或 Cookie / 访问令牌及 API User ID".to_string())
            }
        }
        _ => Ok(()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCheckInPolicyPreview {
    pub method: ProviderCheckInMethod,
    pub method_label: &'static str,
    pub configurable: bool,
    pub supported: bool,
    pub message: String,
}

pub fn preview(provider: &Provider) -> ProviderCheckInPolicyPreview {
    let method = effective_method(provider);
    let method_label = match method {
        ProviderCheckInMethod::Auto | ProviderCheckInMethod::Standard => "标准签到",
        ProviderCheckInMethod::SessionSignIn => "会话签到",
        ProviderCheckInMethod::FreshLogin => "重新登录签到",
    };
    let validation = validate_credentials(provider);
    let message = match validation.as_ref() {
        Err(message) => message.clone(),
        Ok(()) => {
            let source = if provider.automation.check_in_method != ProviderCheckInMethod::Auto {
                "使用手动选择的方式"
            } else if preset_for_url(&provider.identity.base_url).is_some() {
                "已匹配站点预设"
            } else {
                "自动使用标准 NewAPI 签到"
            };
            let detail = match method {
                ProviderCheckInMethod::FreshLogin => {
                    "每次重新登录并确认账号状态；奖励是否到账以站点记录为准"
                }
                ProviderCheckInMethod::SessionSignIn => {
                    "使用 session Cookie 提交签到，无需 API User ID"
                }
                _ => "先读取今日状态，未签到时再提交",
            };
            format!("{source}。{detail}。")
        }
    };
    ProviderCheckInPolicyPreview {
        method,
        method_label,
        configurable: provider.identity.protocol == ProviderProtocol::NewApi
            && provider.auth.mode != AuthMode::ApiKey,
        supported: validation.is_ok(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    #[test]
    fn manual_method_overrides_presets_and_works_on_unfamiliar_hosts() {
        let mut provider = Provider::from_input(ProviderInput::default(), "fixture".into());
        provider.identity.base_url = "https://anyrouter.top".into();
        assert_eq!(
            effective_method(&provider),
            ProviderCheckInMethod::SessionSignIn
        );
        provider.automation.check_in_method = ProviderCheckInMethod::Standard;
        assert_eq!(effective_method(&provider), ProviderCheckInMethod::Standard);
        provider.identity.base_url = "https://example.test/relay".into();
        provider.automation.check_in_method = ProviderCheckInMethod::FreshLogin;
        assert_eq!(
            effective_method(&provider),
            ProviderCheckInMethod::FreshLogin
        );
        assert!(!preview(&provider).supported);
        provider.auth.login_username = "fixture".into();
        provider.auth.login_password = "fixture-password".into();
        assert!(preview(&provider).supported);
    }

    #[test]
    fn preset_matches_host_not_path_or_hostname_substring() {
        assert_eq!(preset_for_url("https://example.test/agentrouter.org"), None);
        assert_eq!(preset_for_url("https://anyrouter.top.example.test"), None);
        assert_eq!(
            preset_for_url("https://api.agentrouter.org"),
            Some(ProviderCheckInMethod::FreshLogin)
        );
    }
}
