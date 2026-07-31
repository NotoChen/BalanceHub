use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{auth::authenticate_account, keys::fetch_api_keys, profile::user_login_name},
        transport::build_client,
    },
    models::{
        AppSettings, AuthMode, Provider, ProviderApiKeyOption, ProviderCredentialCompletionResult,
        ProviderCredentialCompletionStep, ProviderInput, ProviderProtocol,
    },
};

impl Sub2ApiAdapter {
    pub(crate) async fn complete_credentials(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
        provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        let mut updated = input;
        if matches!(updated.auth.mode, AuthMode::Session) {
            return Err(
                "Sub2API 不使用 Cookie 认证，请改用账号密码、访问令牌或 API Key".to_string(),
            );
        }
        if matches!(updated.auth.mode, AuthMode::ApiKey) {
            let key_available = !updated.auth.api_key.trim().is_empty();
            return Ok(ProviderCredentialCompletionResult {
                api_key_options: if !key_available {
                    Vec::new()
                } else {
                    vec![ProviderApiKeyOption::current_for_protocol(
                        &updated.auth.api_key,
                        ProviderProtocol::Sub2Api,
                    )]
                },
                input: updated,
                changed_fields: Vec::new(),
                steps: vec![step(
                    "API Key",
                    key_available,
                    if key_available {
                        "API Key 仅用于调用模型，不具备账号管理权限"
                    } else {
                        "请填写 API Key"
                    },
                )],
            });
        }

        let client = build_client(
            settings,
            &Provider::from_input(updated.clone(), provider_id.clone()),
        )?;
        let provider = Provider::from_input(updated.clone(), provider_id);
        let (mut authenticated, user) = authenticate_account(&client, &provider).await?;
        let mut changed_fields = Vec::new();
        sync_authenticated_tokens(&mut updated, &authenticated, &mut changed_fields);
        if updated.auth.login_username.trim().is_empty() {
            if let Some(login) = user_login_name(&user) {
                updated.auth.login_username = login;
                changed_fields.push("loginUsername".to_string());
            }
        }
        let mut steps = vec![if matches!(updated.auth.mode, AuthMode::Password) {
            step(
                "账号密码 -> 访问令牌",
                true,
                "已获取并缓存 Sub2API 访问令牌",
            )
        } else {
            step("验证访问令牌", true, "Sub2API 访问令牌有效")
        }];

        let (key_authenticated, options) = match fetch_api_keys(&client, &authenticated).await {
            Ok(result) => result,
            Err(message) => {
                steps_for_key_query_failure(&mut steps, &updated, &message);
                return Ok(ProviderCredentialCompletionResult {
                    input: updated,
                    changed_fields,
                    steps,
                    api_key_options: Vec::new(),
                });
            }
        };
        authenticated = key_authenticated;
        sync_authenticated_tokens(&mut updated, &authenticated, &mut changed_fields);
        if options.is_empty() {
            if updated.auth.api_key.trim().is_empty() {
                steps.push(step(
                    if matches!(updated.auth.mode, AuthMode::Password) {
                        "访问令牌 -> API Key"
                    } else {
                        "验证访问令牌 -> API Key"
                    },
                    false,
                    "站点没有已有 API Key；保存后可在密钥管理中创建",
                ));
            } else {
                steps.push(step(
                    if matches!(updated.auth.mode, AuthMode::Password) {
                        "访问令牌 -> API Key"
                    } else {
                        "验证访问令牌 -> API Key"
                    },
                    true,
                    "保留手动填写的 API Key",
                ));
            }
        } else {
            let current = updated.auth.api_key.trim().to_string();
            let selected = options
                .iter()
                .find(|option| option.key_available && option.key == current)
                .cloned();
            if updated.auth.api_key.trim().is_empty() {
                let usable = options
                    .iter()
                    .filter(|option| option.key_available)
                    .collect::<Vec<_>>();
                if usable.len() == 1 {
                    updated.auth.api_key = usable[0].key.clone();
                    updated.auth.api_key_token_id = usable[0].token_id.clone();
                    changed_fields.push("apiKey".to_string());
                    changed_fields.push("apiKeyTokenId".to_string());
                }
            } else if let Some(selected) = selected {
                updated.auth.api_key_token_id = selected.token_id;
            }
            updated.auth.api_key_options = options.clone();
            changed_fields.push("apiKeyOptions".to_string());
            steps.push(step(
                if matches!(updated.auth.mode, AuthMode::Password) {
                    "访问令牌 -> API Key"
                } else {
                    "验证访问令牌 -> API Key"
                },
                true,
                format!("已同步 {} 个 API Key", options.len()),
            ));
        }

        Ok(ProviderCredentialCompletionResult {
            input: updated,
            changed_fields,
            steps,
            api_key_options: options,
        })
    }
}

fn step(
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

fn steps_for_key_query_failure(
    steps: &mut Vec<ProviderCredentialCompletionStep>,
    input: &ProviderInput,
    message: &str,
) {
    let label = if matches!(input.auth.mode, AuthMode::Password) {
        "访问令牌 -> API Key"
    } else {
        "验证访问令牌 -> API Key"
    };
    steps.push(step(
        label,
        false,
        format!("读取 API Key 列表失败：{message}"),
    ));
}

fn sync_authenticated_tokens(
    input: &mut ProviderInput,
    authenticated: &Provider,
    changed_fields: &mut Vec<String>,
) {
    if input.auth.access_token != authenticated.auth.access_token {
        input.auth.access_token = authenticated.auth.access_token.clone();
        push_changed_field(changed_fields, "accessToken");
    }
    if input.auth.refresh_token != authenticated.auth.refresh_token {
        input.auth.refresh_token = authenticated.auth.refresh_token.clone();
        push_changed_field(changed_fields, "refreshToken");
    }
    if input.auth.access_token_expires_at != authenticated.auth.access_token_expires_at {
        input.auth.access_token_expires_at = authenticated.auth.access_token_expires_at;
        push_changed_field(changed_fields, "accessTokenExpiresAt");
    }
}

fn push_changed_field(changed_fields: &mut Vec<String>, field: &str) {
    if !changed_fields.iter().any(|known| known == field) {
        changed_fields.push(field.to_string());
    }
}
