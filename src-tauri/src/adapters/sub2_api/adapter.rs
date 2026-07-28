//! Sub2API protocol adapter.
//!
//! Sub2API deliberately lives beside the NewAPI implementation instead of
//! sharing its request helpers.  The two protocols use different response
//! envelopes and, more importantly, different authentication semantics:
//! Sub2API uses JWTs for account APIs and API Keys only for gateway APIs.

use crate::adapters::transport::build_client;
use crate::models::{
    AppSettings, AuthMode, Provider, ProviderApiKeyOption, ProviderCapabilities,
    ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderConnectionTestResult,
    ProviderConnectionTestStep, ProviderCredentialCompletionResult,
    ProviderCredentialCompletionStep, ProviderInput, ProviderProtocol, ProviderQuotaDisplay,
    ProviderRequestLogStats, ProviderRequestLogsQuery, ProviderRequestLogsResult,
    ProviderSiteProbeResult, ProviderStatus, ProviderUsageModelStat, ProviderUsagePoint,
    ProviderUsageSummary,
};
use reqwest::{Client, Method};
use serde_json::{json, Value};

use super::{
    auth::{
        authenticate_account, is_refresh_chain_broken, needs_token_refresh, refresh_tokens,
        request_account_json,
    },
    json::{integer_field, number_field, object_array, string_field},
    keys::{api_key_from_value, fetch_api_keys},
    profile::{apply_user, fetch_models, fetch_site, quota_display, user_login_name},
    response::normalize_base_url,
    usage::{normalize_log, urlencoding, usage_dates},
};

pub(crate) struct Sub2ApiAdapter;

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

    pub(crate) async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderConnectionTestResult, String> {
        let client = build_client(settings, provider)?;
        let site = fetch_site(&client, &provider.identity.base_url).await.ok();
        let mut steps = Vec::new();

        // 测试连接只做只读验证，不能使用会轮换的 refresh_token。Sub2API 每次刷新都会
        // 立即作废旧 token，而连接测试的返回结构不负责持久化新 token；若在这里轮换，
        // 存储中就会留下已失效的旧 token。访问令牌失效时仍可用账号密码临时重登验证。
        let mut connection_provider = provider.clone();
        connection_provider.auth.refresh_token.clear();
        let refreshed = self
            .refresh_provider_with_client(&client, &connection_provider, site)
            .await;
        match refreshed.runtime.status {
            ProviderStatus::Error => Ok(ProviderConnectionTestResult {
                ok: false,
                message: refreshed
                    .runtime
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "连接失败".to_string()),
                available: None,
                used: None,
                quota_display: quota_display(&refreshed),
                steps,
            }),
            _ => {
                let available = refreshed.quota.known.then_some(refreshed.quota.available);
                let used = refreshed.quota.total_known.then_some(refreshed.quota.used);
                steps.push(ProviderConnectionTestStep {
                    name: "协议连接".to_string(),
                    ok: true,
                    message: if refreshed.quota.known {
                        "Sub2API 账号已连接，余额已读取".to_string()
                    } else {
                        "Sub2API API Key 已连接；该认证方式不公开账号余额".to_string()
                    },
                    available,
                    used,
                    quota_display: quota_display(&refreshed),
                });
                Ok(ProviderConnectionTestResult {
                    ok: true,
                    message: steps[0].message.clone(),
                    available,
                    used,
                    quota_display: quota_display(&refreshed),
                    steps,
                })
            }
        }
    }

    pub(crate) async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        let client = build_client(settings, provider)?;
        match fetch_site(&client, &provider.identity.base_url).await {
            Ok(site) => Ok(ProviderSiteProbeResult {
                ok: true,
                message: "Sub2API 站点可访问，已读取公开信息".to_string(),
                system_name: string_field(&site, &["site_name", "siteName", "name"]),
                logo: string_field(&site, &["site_logo", "siteLogo", "logo"]),
                quota_display: ProviderQuotaDisplay::default(),
            }),
            Err(message) => Ok(ProviderSiteProbeResult {
                ok: false,
                message,
                system_name: None,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            }),
        }
    }

    pub(crate) async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<Vec<ProviderApiKeyOption>, String> {
        let client = build_client(settings, provider)?;
        let (_authenticated, options) = fetch_api_keys(&client, provider).await?;
        Ok(options)
    }

    pub(crate) async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<ProviderApiKeyOption, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("请填写 API 密钥名称".to_string());
        }
        let client = build_client(settings, provider)?;
        let (_authenticated, data) = request_account_json(
            &client,
            provider,
            Method::POST,
            "/keys",
            Some(json!({"name": name})),
            "创建 API Key",
        )
        .await?;
        api_key_from_value(&data).ok_or_else(|| "创建成功但响应中没有返回 API Key".to_string())
    }

    pub(crate) async fn generate_access_token(
        &self,
        _settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        if !provider.auth.access_token.trim().is_empty() {
            return Ok(provider.auth.access_token.clone());
        }
        Err("Sub2API 的访问令牌由账号密码登录自动生成，不能通过 API Key 创建".to_string())
    }

    pub(crate) async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<(), String> {
        let token_id = token_id.trim();
        if token_id.is_empty() {
            return Err("缺少 API Key ID".to_string());
        }
        let client = build_client(settings, provider)?;
        request_account_json(
            &client,
            provider,
            Method::DELETE,
            &format!("/keys/{token_id}"),
            None,
            "删除 API Key",
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<ProviderUsageSummary, String> {
        let client = build_client(settings, provider)?;
        let (start, end) = usage_dates(period);
        let query = format!("?start_date={start}&end_date={end}");
        let (authenticated, trend) = request_account_json(
            &client,
            provider,
            Method::GET,
            &format!("/usage/dashboard/trend{query}"),
            None,
            "读取用量趋势",
        )
        .await?;
        let (_authenticated, models) = request_account_json(
            &client,
            &authenticated,
            Method::GET,
            &format!("/usage/dashboard/models{query}"),
            None,
            "读取模型用量",
        )
        .await?;

        let points = object_array(&trend, "trend")
            .into_iter()
            .map(|item| ProviderUsagePoint {
                date: string_field(&item, &["date"]).unwrap_or_default(),
                used: number_field(&item, &["actual_cost", "cost"]),
                request_count: integer_field(&item, &["requests"]),
                token_used: integer_field(&item, &["total_tokens"]),
            })
            .collect::<Vec<_>>();
        let model_stats = object_array(&models, "models")
            .into_iter()
            .map(|item| ProviderUsageModelStat {
                model_name: string_field(&item, &["model"])
                    .unwrap_or_else(|| "未知模型".to_string()),
                used: number_field(&item, &["actual_cost", "cost"]),
                request_count: integer_field(&item, &["requests"]),
                token_used: integer_field(&item, &["total_tokens"]),
            })
            .collect::<Vec<_>>();
        Ok(ProviderUsageSummary {
            provider_id: provider.identity.id.clone(),
            provider_name: provider.identity.name.clone(),
            quota_display: quota_display(provider),
            points,
            model_stats,
            // Sub2API only exposes aggregate daily trend and aggregate model
            // statistics. Do not fabricate per-model daily zero points.
            model_points: Vec::new(),
        })
    }

    pub(crate) async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        let client = build_client(settings, provider)?;
        let page = query.page + 1;
        let mut path = format!("/usage?page={page}&page_size={}", query.page_size.max(1));
        if !query.keyword.trim().is_empty() {
            path.push_str("&model=");
            path.push_str(&urlencoding(query.keyword.trim()));
        }
        let (_authenticated, data) =
            request_account_json(&client, provider, Method::GET, &path, None, "读取请求日志")
                .await?;
        let logs = object_array(&data, "items")
            .into_iter()
            .map(normalize_log)
            .collect::<Vec<_>>();
        let total = data.get("total").map(|_| integer_field(&data, &["total"]));
        Ok(ProviderRequestLogsResult {
            provider_id: provider.identity.id.clone(),
            provider_name: provider.identity.name.clone(),
            page: query.page,
            page_size: query.page_size,
            total,
            quota_display: quota_display(provider),
            stats: ProviderRequestLogStats::default(),
            logs,
            message: "请求日志已加载".to_string(),
        })
    }

    pub(crate) async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<String, String> {
        if password.trim().is_empty() {
            return Err("请输入新密码".to_string());
        }
        let client = build_client(settings, provider)?;
        request_account_json(
            &client,
            provider,
            Method::PUT,
            "/user/password",
            Some(json!({
                "old_password": original_password,
                "new_password": password.trim(),
            })),
            "修改 Sub2API 密码",
        )
        .await?;
        Ok("密码已更新".to_string())
    }

    pub(crate) async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(ProviderCapabilities, String, Option<String>), String> {
        let mut capabilities = ProviderCapabilities {
            check_in_known: true,
            check_in_supported: false,
            api_key_management_known: true,
            api_key_management_supported: false,
            invitation_known: true,
            invitation_supported: false,
            ..Default::default()
        };
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Ok((capabilities, String::new(), None));
        }
        let client = build_client(settings, provider)?;
        let mut errors = Vec::new();
        let mut authenticated = provider.clone();
        match fetch_api_keys(&client, provider).await {
            Ok((next, _)) => {
                authenticated = next;
                capabilities.api_key_management_supported = true;
            }
            Err(message) => errors.push(format!("密钥管理: {message}")),
        }
        let invite = match self.invite_link_with_client(&client, &authenticated).await {
            Ok(link) => {
                capabilities.invitation_supported = true;
                link
            }
            Err(message) => {
                errors.push(format!("邀请链接: {message}"));
                String::new()
            }
        };
        Ok((
            capabilities,
            invite,
            (!errors.is_empty()).then(|| errors.join("；")),
        ))
    }

    pub(crate) async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        let client = build_client(settings, provider)?;
        self.invite_link_with_client(&client, provider).await
    }

    async fn invite_link_with_client(
        &self,
        client: &Client,
        provider: &Provider,
    ) -> Result<String, String> {
        let (_authenticated, data) = request_account_json(
            client,
            provider,
            Method::GET,
            "/user/aff",
            None,
            "读取邀请链接",
        )
        .await?;
        let code = string_field(&data, &["aff_code", "affCode", "code"])
            .ok_or_else(|| "Sub2API 没有返回邀请编码".to_string())?;
        Ok(format!(
            "{}/register?aff_code={}",
            normalize_base_url(&provider.identity.base_url),
            urlencoding(&code)
        ))
    }

    pub(crate) async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Provider {
        match build_client(settings, provider) {
            Ok(client) => {
                self.refresh_provider_with_client(&client, provider, None)
                    .await
            }
            Err(message) => provider_with_error(provider, message),
        }
    }

    pub(crate) async fn check_in(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
    ) -> Result<ProviderCheckInResult, String> {
        Err("Sub2API 不提供用户签到接口".to_string())
    }

    pub(crate) async fn check_in_records(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
        _month: &str,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        Err("Sub2API 不提供签到记录接口".to_string())
    }

    async fn refresh_provider_with_client(
        &self,
        client: &Client,
        provider: &Provider,
        site_hint: Option<Value>,
    ) -> Provider {
        let mut next = provider.clone();
        if !provider.runtime.enabled {
            return next;
        }

        // 唯一使用 refresh_token 的地方：持久化刷新路径（配合刷新闸门单飞），避免
        // 已轮换的 refresh_token 被重复提交而触发服务端「重用攻击」吊销整个会话家族。
        let mut working = provider.clone();
        if needs_token_refresh(&working) {
            match refresh_tokens(client, &working).await {
                Ok(refreshed) => working = refreshed,
                Err(err) if is_refresh_chain_broken(&err) => {
                    // 刷新链已断（过期/吊销/重用）：清空令牌，下面回退账号密码登录（无密码则报错）。
                    working.auth.access_token = String::new();
                    working.auth.refresh_token = String::new();
                    working.auth.access_token_expires_at = None;
                }
                // 瞬时失败：保留 refresh_token，本轮沿用旧令牌，交给后续认证或下一轮重试。
                Err(_) => {}
            }
        }
        next.auth.access_token = working.auth.access_token.clone();
        next.auth.refresh_token = working.auth.refresh_token.clone();
        next.auth.access_token_expires_at = working.auth.access_token_expires_at;

        let site = match site_hint {
            Some(value) => Some(value),
            None => fetch_site(client, &provider.identity.base_url).await.ok(),
        };
        if let Some(site) = site.as_ref() {
            if let Some(name) = string_field(site, &["site_name", "siteName", "name"]) {
                next.identity.name = name;
            }
            if let Some(logo) = string_field(site, &["site_logo", "siteLogo", "logo"]) {
                next.identity.site_logo = logo;
            }
        }

        match authenticate_account(client, &working).await {
            Ok((authenticated, user)) => {
                if !authenticated.auth.access_token.trim().is_empty() {
                    next.auth.access_token = authenticated.auth.access_token.clone();
                }
                if !authenticated.auth.refresh_token.trim().is_empty() {
                    next.auth.refresh_token = authenticated.auth.refresh_token.clone();
                }
                if authenticated.auth.access_token_expires_at.is_some() {
                    next.auth.access_token_expires_at = authenticated.auth.access_token_expires_at;
                }
                apply_user(&mut next, &user);
                next.runtime.status = if next.quota.available <= 0.0 {
                    ProviderStatus::Warning
                } else {
                    ProviderStatus::Ok
                };
                next.quota.known = true;
                next.quota.total_known = false;
                next.quota.unlimited = false;
                next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                next.runtime.error_message = None;
                next
            }
            Err(_message) if matches!(provider.auth.mode, AuthMode::ApiKey) => {
                match fetch_models(client, provider).await {
                    Ok(_) => {
                        next.quota.known = false;
                        next.quota.total_known = false;
                        next.quota.available = 0.0;
                        next.quota.used = 0.0;
                        next.quota.unlimited = false;
                        next.quota.scope = crate::models::ProviderQuotaScope::Token;
                        next.runtime.status = ProviderStatus::Ok;
                        next.runtime.error_message = None;
                        next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                        next
                    }
                    Err(model_error) => provider_with_error(&next, model_error),
                }
            }
            Err(message) => provider_with_error(&next, message),
        }
    }
}

fn provider_with_error(provider: &Provider, message: String) -> Provider {
    let mut next = provider.clone();
    next.runtime.status = ProviderStatus::Error;
    next.runtime.error_message = Some(message);
    next
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
