use crate::models::{
    normalize_api_key, Provider, ProviderApiKeyOption, ProviderCredentialCompletionResult,
    ProviderCredentialCompletionStep, ProviderInput,
};
use reqwest::{Client, Method};
use serde_json::Value;
use std::collections::BTreeSet;

use super::newapi_http::{
    build_url, build_user_request, is_anyrouter_base_url, normalize_base_url, UserCredential,
};
use super::newapi_keys::fetch_api_key_options;
use super::newapi_response::{extract_string_field, parse_success_data, send_text};
use super::newapi_session::decode_session_user_id;

pub async fn complete_credentials(
    client: &Client,
    input: ProviderInput,
) -> Result<ProviderCredentialCompletionResult, String> {
    let mut updated = input;
    let base_url = normalize_base_url(&updated.identity.base_url);
    if base_url.is_empty() {
        return Err("请先填写中转站地址".to_string());
    }

    let is_anyrouter = is_anyrouter_base_url(&base_url);
    let mut changed_fields = BTreeSet::new();
    let mut steps = Vec::new();
    let mut api_key_options = Vec::new();

    if fill_api_user_from_session_cookie(&mut updated) {
        changed_fields.insert("apiUser".to_string());
        steps.push(completion_step(
            "会话 Cookie -> API User ID",
            true,
            "已从会话 Cookie 解析出用户 ID",
        ));
    }

    if updated.auth.access_token.trim().is_empty() {
        if updated.auth.session_cookie.trim().is_empty() {
            steps.push(completion_step(
                "会话 Cookie -> 访问令牌",
                false,
                "未填写会话 Cookie，跳过访问令牌补全",
            ));
        } else if updated.auth.api_user.trim().is_empty() {
            steps.push(completion_step(
                "会话 Cookie -> 访问令牌",
                false,
                "缺少 API User ID，NewAPI 用户接口需要 New-Api-User",
            ));
        } else {
            match fetch_user_self(
                client,
                &base_url,
                &updated.auth.api_user,
                UserCredential::Session(updated.auth.session_cookie.clone()),
                is_anyrouter,
            )
            .await
            {
                Ok(data) => {
                    if fill_api_user_from_self(&mut updated, &data) {
                        changed_fields.insert("apiUser".to_string());
                    }

                    if let Some(access_token) =
                        extract_string_field(&data, &["access_token", "accessToken"])
                    {
                        if !access_token.trim().is_empty() {
                            updated.auth.access_token = access_token;
                            changed_fields.insert("accessToken".to_string());
                            steps.push(completion_step(
                                "会话 Cookie -> 访问令牌",
                                true,
                                "已从用户信息补全访问令牌",
                            ));
                        } else {
                            steps.push(completion_step(
                                "会话 Cookie -> 访问令牌",
                                false,
                                "用户信息中没有可用访问令牌",
                            ));
                        }
                    } else {
                        steps.push(completion_step(
                            "会话 Cookie -> 访问令牌",
                            false,
                            "用户信息没有返回访问令牌；可手动填写现有令牌，或确认后生成新令牌（旧令牌会失效）",
                        ));
                    }
                }
                Err(message) => {
                    steps.push(completion_step(
                        "会话 Cookie -> 访问令牌",
                        false,
                        format!("补全失败: {message}"),
                    ));
                }
            }
        }
    } else {
        steps.push(completion_step(
            "会话 Cookie -> 访问令牌",
            true,
            "访问令牌已存在，未覆盖",
        ));
    }

    if updated.auth.api_key.trim().is_empty() {
        let credential = if !updated.auth.session_cookie.trim().is_empty() {
            Some(UserCredential::Session(updated.auth.session_cookie.clone()))
        } else if !updated.auth.access_token.trim().is_empty() {
            Some(UserCredential::AccessToken(
                updated.auth.access_token.clone(),
            ))
        } else {
            None
        };

        if updated.auth.api_user.trim().is_empty() {
            steps.push(completion_step(
                "访问令牌 -> API 密钥",
                false,
                "缺少 API User ID，无法读取 API 密钥列表",
            ));
        } else if let Some(credential) = credential {
            match fetch_api_key_options(
                client,
                &base_url,
                &updated.auth.api_user,
                credential.clone(),
                is_anyrouter,
            )
            .await
            {
                Ok(options) if !options.is_empty() => {
                    api_key_options = options;
                    updated.auth.api_key = api_key_options[0].key.clone();
                    changed_fields.insert("apiKey".to_string());
                    steps.push(completion_step(
                        "访问令牌 -> API 密钥",
                        true,
                        format!(
                            "已补全 API 密钥，可选 {} 个，默认使用第一个",
                            api_key_options.len()
                        ),
                    ));
                }
                Ok(_) => {
                    steps.push(completion_step(
                        "访问令牌 -> API 密钥",
                        false,
                        "没有可用 API 密钥，请在密钥管理中创建并填写自定义名称",
                    ));
                }
                Err(message) => {
                    steps.push(completion_step(
                        "访问令牌 -> API 密钥",
                        false,
                        format!("补全失败: {message}"),
                    ));
                }
            }
        } else {
            steps.push(completion_step(
                "访问令牌 -> API 密钥",
                false,
                "缺少访问令牌或会话 Cookie，无法读取 API 密钥",
            ));
        }
    } else {
        steps.push(completion_step(
            "访问令牌 -> API 密钥",
            true,
            "API 密钥已存在，未覆盖",
        ));
        api_key_options.push(ProviderApiKeyOption {
            name: "当前 API 密钥".to_string(),
            key: normalize_api_key(&updated.auth.api_key),
            token_id: String::new(),
            status: String::new(),
            used_quota: 0.0,
            remain_quota: 0.0,
            unlimited_quota: false,
            group: String::new(),
            model_limits_enabled: false,
            model_limits: Vec::new(),
            allow_ips: Vec::new(),
            created_time: None,
            accessed_time: None,
            expired_time: None,
        });
    }

    Ok(ProviderCredentialCompletionResult {
        input: updated,
        changed_fields: changed_fields.into_iter().collect(),
        steps,
        api_key_options,
    })
}

fn fill_api_user_from_session_cookie(input: &mut ProviderInput) -> bool {
    if input.auth.session_cookie.trim().is_empty() {
        return false;
    }

    let Some(api_user) = decode_session_user_id(&input.auth.session_cookie) else {
        return false;
    };

    if input.auth.api_user.trim() == api_user {
        return false;
    }

    input.auth.api_user = api_user;
    true
}

pub async fn create_access_token(client: &Client, provider: &Provider) -> Result<String, String> {
    let base_url = normalize_base_url(&provider.identity.base_url);
    if base_url.is_empty() {
        return Err("缺少中转站地址".to_string());
    }
    let api_user = provider.auth.api_user.trim();
    if api_user.is_empty() {
        return Err("缺少 API User ID，无法生成访问令牌".to_string());
    }
    if provider.auth.session_cookie.trim().is_empty() {
        return Err("缺少会话 Cookie，无法生成访问令牌".to_string());
    }
    generate_access_token(
        client,
        &base_url,
        api_user,
        provider.auth.session_cookie.clone(),
        is_anyrouter_base_url(&base_url),
    )
    .await
}

async fn fetch_user_self(
    client: &Client,
    base_url: &str,
    api_user: &str,
    credential: UserCredential,
    is_anyrouter: bool,
) -> Result<Value, String> {
    let url = build_url(base_url, "/api/user/self")?;
    let request = build_user_request(
        client,
        Method::GET,
        url,
        base_url,
        api_user,
        credential,
        is_anyrouter,
    )
    .await?;
    let body = send_text(request, "读取用户信息").await?;
    parse_success_data(&body.0, body.1, "用户信息")
}

async fn generate_access_token(
    client: &Client,
    base_url: &str,
    api_user: &str,
    session_cookie: String,
    is_anyrouter: bool,
) -> Result<String, String> {
    let url = build_url(base_url, "/api/user/token")?;
    let request = build_user_request(
        client,
        Method::GET,
        url,
        base_url,
        api_user,
        UserCredential::Session(session_cookie),
        is_anyrouter,
    )
    .await?;
    let (status, body) = send_text(request, "创建访问令牌").await?;
    let data = parse_success_data(&status, body, "创建访问令牌")?;

    data.as_str()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .or_else(|| extract_string_field(&data, &["access_token", "accessToken", "token"]))
        .ok_or_else(|| "接口没有返回访问令牌".to_string())
}

fn fill_api_user_from_self(input: &mut ProviderInput, data: &Value) -> bool {
    if !input.auth.api_user.trim().is_empty() {
        return false;
    }

    let Some(id) = data.get("id") else {
        return false;
    };

    let api_user = if let Some(id) = id.as_i64() {
        id.to_string()
    } else if let Some(id) = id.as_str() {
        id.trim().to_string()
    } else {
        String::new()
    };

    if api_user.is_empty() {
        return false;
    }

    input.auth.api_user = api_user;
    true
}

fn completion_step(
    name: impl Into<String>,
    ok: bool,
    message: impl Into<String>,
) -> ProviderCredentialCompletionStep {
    ProviderCredentialCompletionStep {
        name: name.into(),
        ok,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthMode, ProviderProxyMode};

    const SYNTHETIC_SESSION_COOKIE: &str =
        "MHxBUVp6ZEhKcGJtY0FBQUFDYVdRRGFXNTBBQUFBL21CeXxzeW50aGV0aWMtc2lnbmF0dXJl";

    fn provider_input_with_session(api_user: &str) -> ProviderInput {
        ProviderInput {
            identity: crate::models::ProviderIdentityInput {
                name: "Relay".to_string(),
                base_url: "https://relay.example.com".to_string(),
            },
            auth: crate::models::ProviderAuth {
                mode: AuthMode::Session,
                session_cookie: SYNTHETIC_SESSION_COOKIE.to_string(),
                api_user: api_user.to_string(),
                ..ProviderInput::default().auth
            },
            proxy: crate::models::ProviderProxy {
                mode: ProviderProxyMode::Inherit,
                url: String::new(),
            },
            ..ProviderInput::default()
        }
    }

    #[test]
    fn session_cookie_user_id_overrides_stale_api_user() {
        let mut input = provider_input_with_session("stale-user");

        assert!(fill_api_user_from_session_cookie(&mut input));
        assert_eq!(input.auth.api_user, "12345");
    }

    #[test]
    fn session_cookie_user_id_keeps_matching_api_user() {
        let mut input = provider_input_with_session("12345");

        assert!(!fill_api_user_from_session_cookie(&mut input));
        assert_eq!(input.auth.api_user, "12345");
    }
}
