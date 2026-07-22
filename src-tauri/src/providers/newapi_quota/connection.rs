use crate::models::{
    AuthMode, Provider, ProviderConnectionTestResult, ProviderConnectionTestStep,
    ProviderQuotaDisplay,
};
use reqwest::Client;

use super::fetch_quota;
use crate::providers::newapi_http::login_password_provider;

pub async fn test_connection(
    client: &Client,
    provider: &Provider,
) -> Result<ProviderConnectionTestResult, String> {
    let mut steps = Vec::new();

    if matches!(provider.auth.mode, AuthMode::Password) {
        let step = match login_password_provider(client, provider).await {
            Ok(authenticated) => match fetch_quota(client, &authenticated).await {
                Ok(profile) => ProviderConnectionTestStep {
                    name: "账号密码".to_string(),
                    ok: true,
                    message: "登录和余额接口可用".to_string(),
                    available: Some(profile.available),
                    used: Some(profile.used),
                    quota_display: profile.quota_display,
                },
                Err(message) => ProviderConnectionTestStep {
                    name: "账号密码".to_string(),
                    ok: false,
                    message,
                    available: None,
                    used: None,
                    quota_display: ProviderQuotaDisplay::default(),
                },
            },
            Err(message) => ProviderConnectionTestStep {
                name: "账号密码".to_string(),
                ok: false,
                message,
                available: None,
                used: None,
                quota_display: ProviderQuotaDisplay::default(),
            },
        };
        return Ok(ProviderConnectionTestResult {
            ok: step.ok,
            message: if step.ok {
                "账号密码测试通过".to_string()
            } else {
                "测试未通过".to_string()
            },
            available: step.available,
            used: step.used,
            quota_display: step.quota_display.clone(),
            steps: vec![step],
        });
    }

    if matches!(provider.auth.mode, AuthMode::ApiKey) {
        let step = test_connection_with_auth(client, provider, AuthMode::ApiKey).await;
        return Ok(ProviderConnectionTestResult {
            ok: step.ok,
            message: if step.ok {
                "API 密钥测试通过".to_string()
            } else {
                "测试未通过".to_string()
            },
            available: step.available,
            used: step.used,
            quota_display: step.quota_display.clone(),
            steps: vec![step],
        });
    }

    if !provider.auth.session_cookie.trim().is_empty() {
        steps.push(test_connection_with_auth(client, provider, AuthMode::Session).await);
    } else {
        steps.push(skipped_test_step("会话 Cookie", "未配置，跳过"));
    }

    if !provider.auth.access_token.trim().is_empty() {
        steps.push(test_connection_with_auth(client, provider, AuthMode::AccessToken).await);
    } else {
        steps.push(skipped_test_step("访问令牌", "未配置，跳过"));
    }

    if !provider.auth.api_key.trim().is_empty() {
        steps.push(test_connection_with_auth(client, provider, AuthMode::ApiKey).await);
    } else {
        steps.push(skipped_test_step("API 密钥", "未配置，跳过"));
    }

    let first_success = steps.iter().find(|step| step.ok);
    if let Some(step) = first_success {
        Ok(ProviderConnectionTestResult {
            ok: true,
            message: format!("{}测试通过", step.name),
            available: step.available,
            used: step.used,
            quota_display: step.quota_display.clone(),
            steps,
        })
    } else {
        Ok(ProviderConnectionTestResult {
            ok: false,
            message: "测试未通过".to_string(),
            available: None,
            used: None,
            quota_display: ProviderQuotaDisplay::default(),
            steps,
        })
    }
}

fn skipped_test_step(name: &str, message: &str) -> ProviderConnectionTestStep {
    ProviderConnectionTestStep {
        name: name.to_string(),
        ok: false,
        message: message.to_string(),
        available: None,
        used: None,
        quota_display: ProviderQuotaDisplay::default(),
    }
}

async fn test_connection_with_auth(
    client: &Client,
    provider: &Provider,
    auth_mode: AuthMode,
) -> ProviderConnectionTestStep {
    let mut testing_provider = provider.clone();
    testing_provider.auth.mode = auth_mode;
    isolate_test_credentials(&mut testing_provider, auth_mode);
    let name = match auth_mode {
        AuthMode::Session => "会话 Cookie",
        AuthMode::AccessToken => "访问令牌",
        AuthMode::ApiKey => "API 密钥",
        AuthMode::Password => "账号密码",
    }
    .to_string();

    match fetch_quota(client, &testing_provider).await {
        Ok(profile) => ProviderConnectionTestStep {
            name,
            ok: true,
            message: "余额接口可用".to_string(),
            available: Some(profile.available),
            used: Some(profile.used),
            quota_display: profile.quota_display,
        },
        Err(message) => ProviderConnectionTestStep {
            name,
            ok: false,
            message,
            available: None,
            used: None,
            quota_display: ProviderQuotaDisplay::default(),
        },
    }
}

pub(super) fn isolate_test_credentials(provider: &mut Provider, auth_mode: AuthMode) {
    match auth_mode {
        AuthMode::Session => {
            provider.auth.access_token.clear();
            provider.auth.api_key.clear();
        }
        AuthMode::AccessToken => {
            provider.auth.session_cookie.clear();
            provider.auth.api_key.clear();
        }
        AuthMode::ApiKey => {
            provider.auth.session_cookie.clear();
            provider.auth.access_token.clear();
            provider.auth.api_user.clear();
        }
        AuthMode::Password => {
            provider.auth.access_token.clear();
            provider.auth.api_key.clear();
        }
    }
}
