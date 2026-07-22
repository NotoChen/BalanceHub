use crate::models::{
    normalize_api_key, AuthMode, Provider, ProviderApiKeyOption,
    ProviderCredentialCompletionResult, ProviderCredentialCompletionStep, ProviderInput,
};
use reqwest::{Client, Method};
use serde_json::Value;
use std::collections::BTreeSet;

use super::newapi_http::{
    build_url, build_user_request, is_anyrouter_base_url, login_password_provider,
    normalize_base_url, UserCredential,
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

    if matches!(updated.auth.mode, crate::models::AuthMode::Password) {
        let provider = Provider::from_input(updated.clone(), "credential-completion".to_string());
        let authenticated = login_password_provider(client, &provider).await?;
        updated.auth.session_cookie = authenticated.auth.session_cookie;
        updated.auth.api_user = authenticated.auth.api_user;
    }
    let mut changed_fields = BTreeSet::new();
    let mut steps = Vec::new();
    let mut api_key_options = Vec::new();
    let mut user_self_fetched = false;

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
            user_self_fetched = true;
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
                    if fill_login_username_from_self(&mut updated, &data) {
                        changed_fields.insert("loginUsername".to_string());
                        steps.push(completion_step(
                            "用户信息同步",
                            true,
                            "已同步用户名/邮箱信息",
                        ));
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
        if updated.auth.api_key.trim().is_empty() {
            steps.push(completion_step(
                "访问令牌 -> API 密钥",
                false,
                "缺少 API User ID，无法读取 API 密钥列表",
            ));
        }
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
            Ok(mut options) if !options.is_empty() => {
                let current_key = normalize_api_key(&updated.auth.api_key);
                let current_token_id = updated.auth.api_key_token_id.trim().to_string();
                let mut cached_options = updated.auth.api_key_options.clone();
                if !current_key.is_empty() {
                    let mut current = ProviderApiKeyOption::current(&current_key);
                    current.token_id = current_token_id.clone();
                    cached_options.push(current);
                }
                ProviderApiKeyOption::merge_cached_key_material(&mut options, &cached_options);
                let selected = options
                    .iter()
                    .find(|option| {
                        option.key_available
                            && !current_token_id.is_empty()
                            && option.token_id == current_token_id
                    })
                    .or_else(|| {
                        (!current_key.is_empty())
                            .then(|| options.iter().find(|option| option.key == current_key))
                            .flatten()
                    })
                    .cloned();

                if let Some(selected) = selected {
                    if updated.auth.api_key != selected.key {
                        updated.auth.api_key = selected.key.clone();
                        changed_fields.insert("apiKey".to_string());
                    }
                    if updated.auth.api_key_token_id != selected.token_id {
                        updated.auth.api_key_token_id = selected.token_id.clone();
                        changed_fields.insert("apiKeyTokenId".to_string());
                    }
                } else if current_key.is_empty() {
                    let usable = options.iter().filter(|option| option.key_available).count();
                    if usable == 1 {
                        if let Some(option) = options.iter().find(|option| option.key_available) {
                            updated.auth.api_key = option.key.clone();
                            updated.auth.api_key_token_id = option.token_id.clone();
                            changed_fields.insert("apiKey".to_string());
                            changed_fields.insert("apiKeyTokenId".to_string());
                        }
                    }
                }

                if !current_key.is_empty()
                    && !options.iter().any(|option| option.key == current_key)
                {
                    let mut current = ProviderApiKeyOption::current(&current_key);
                    current.token_id = current_token_id;
                    options.insert(0, current);
                }
                api_key_options = options;
                updated.auth.api_key_options = api_key_options.clone();
                changed_fields.insert("apiKeyOptions".to_string());
                let usable = api_key_options
                    .iter()
                    .filter(|option| option.key_available)
                    .count();
                let message = if !updated.auth.api_key.trim().is_empty() {
                    format!(
                        "已同步 {} 个 API Key，当前使用已选主 Key",
                        api_key_options.len()
                    )
                } else if usable > 1 {
                    format!(
                        "已同步 {} 个 API Key，请选择一个作为主 Key",
                        api_key_options.len()
                    )
                } else {
                    "已同步 API Key 列表，但没有可读取的完整 Key".to_string()
                };
                steps.push(completion_step("访问令牌 -> API 密钥", true, message));
            }
            Ok(_) => {
                if !updated.auth.api_key.trim().is_empty() {
                    let mut option = ProviderApiKeyOption::current(&updated.auth.api_key);
                    option.token_id = updated.auth.api_key_token_id.clone();
                    api_key_options.push(option);
                    updated.auth.api_key_options = api_key_options.clone();
                    changed_fields.insert("apiKeyOptions".to_string());
                    steps.push(completion_step(
                        "访问令牌 -> API 密钥",
                        true,
                        "站点没有返回其他 API Key，保留当前填写的 Key",
                    ));
                } else {
                    steps.push(completion_step(
                        "访问令牌 -> API 密钥",
                        false,
                        "站点没有已有 API Key，可确认后创建新的 API Key",
                    ));
                }
            }
            Err(message) => {
                steps.push(completion_step(
                    "访问令牌 -> API 密钥",
                    false,
                    format!("读取 API Key 失败: {message}"),
                ));
            }
        }
    } else if updated.auth.api_key.trim().is_empty() {
        steps.push(completion_step(
            "访问令牌 -> API 密钥",
            false,
            "缺少访问令牌或会话 Cookie，无法读取 API 密钥",
        ));
    } else {
        let mut option = ProviderApiKeyOption::current(&updated.auth.api_key);
        option.token_id = updated.auth.api_key_token_id.clone();
        api_key_options.push(option);
        updated.auth.api_key_options = api_key_options.clone();
        changed_fields.insert("apiKeyOptions".to_string());
        steps.push(completion_step(
            "访问令牌 -> API 密钥",
            true,
            "没有可用的用户凭据，保留当前填写的 API Key",
        ));
    }

    // 访问令牌已经存在时，上面的 Cookie -> 用户信息分支不会执行；此时仍可用
    // 当前账号凭据读取用户名/邮箱，作为以后切换到账号密码模式的登录账号。
    if !user_self_fetched
        && updated.auth.login_username.trim().is_empty()
        && !updated.auth.api_user.trim().is_empty()
    {
        if let Some(credential) = user_self_credential(&updated) {
            if let Ok(data) = fetch_user_self(
                client,
                &base_url,
                &updated.auth.api_user,
                credential,
                is_anyrouter,
            )
            .await
            {
                if fill_login_username_from_self(&mut updated, &data) {
                    changed_fields.insert("loginUsername".to_string());
                    steps.push(completion_step(
                        "用户信息同步",
                        true,
                        "已同步用户名/邮箱信息",
                    ));
                }
            }
        }
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

fn fill_login_username_from_self(input: &mut ProviderInput, data: &Value) -> bool {
    if !input.auth.login_username.trim().is_empty() {
        return false;
    }

    let Some(login_username) = extract_string_field(data, &["username", "Username"])
        .or_else(|| extract_string_field(data, &["email", "Email"]))
    else {
        return false;
    };

    input.auth.login_username = login_username;
    true
}

fn user_self_credential(input: &ProviderInput) -> Option<UserCredential> {
    let session_cookie = input.auth.session_cookie.trim();
    let access_token = input.auth.access_token.trim();

    match input.auth.mode {
        AuthMode::AccessToken if !access_token.is_empty() => {
            Some(UserCredential::AccessToken(input.auth.access_token.clone()))
        }
        AuthMode::Session | AuthMode::Password if !session_cookie.is_empty() => {
            Some(UserCredential::Session(input.auth.session_cookie.clone()))
        }
        _ if !session_cookie.is_empty() => {
            Some(UserCredential::Session(input.auth.session_cookie.clone()))
        }
        _ if !access_token.is_empty() => {
            Some(UserCredential::AccessToken(input.auth.access_token.clone()))
        }
        _ => None,
    }
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
                ..crate::models::ProviderIdentityInput::default()
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

    #[test]
    fn login_username_completion_prefers_username_over_email() {
        let mut input = provider_input_with_session("12345");
        let data = serde_json::json!({
            "username": "alice",
            "email": "alice@example.com",
        });

        assert!(fill_login_username_from_self(&mut input, &data));
        assert_eq!(input.auth.login_username, "alice");
    }

    #[test]
    fn login_username_completion_falls_back_to_email() {
        let mut input = provider_input_with_session("12345");
        let data = serde_json::json!({"email": "alice@example.com"});

        assert!(fill_login_username_from_self(&mut input, &data));
        assert_eq!(input.auth.login_username, "alice@example.com");
    }

    #[test]
    fn login_username_completion_preserves_manual_value() {
        let mut input = provider_input_with_session("12345");
        input.auth.login_username = "manual-account".to_string();
        let data = serde_json::json!({"username": "alice", "email": "alice@example.com"});

        assert!(!fill_login_username_from_self(&mut input, &data));
        assert_eq!(input.auth.login_username, "manual-account");
    }
}
