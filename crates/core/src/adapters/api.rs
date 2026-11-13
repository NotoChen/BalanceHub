//! Adapter for OpenAI-compatible gateways whose account protocol is unknown.
//!
//! A generic gateway is intentionally key-scoped. It can be used for model
//! calls and model discovery, but it must never be treated as a NewAPI or
//! Sub2API account endpoint merely because the user supplied an API key.

mod protocol;

use crate::models::provider_domain::failure::ProviderFailureKind;
use crate::{
    adapters::{
        error::AdapterError,
        quota_spec::{apply_auth_scheme, fetch_quota, resolve_provider_url},
        transport::{build_client, ProviderTransport, USER_AGENT_VALUE},
    },
    limits,
    models::{
        is_full_api_key_value, AppSettings, AuthMode, Provider, ProviderApiKeyOption,
        ProviderCapabilities, ProviderConnectionTestResult, ProviderConnectionTestStep,
        ProviderCredentialCompletionResult, ProviderCredentialCompletionStep, ProviderInput,
        ProviderOptionalCapability, ProviderOptionalCapabilityState, ProviderPricingSnapshot,
        ProviderQuotaDisplay, ProviderQuotaScope, ProviderSiteProbeResult, ProviderStatus,
    },
};
use reqwest::{header::ACCEPT, header::RETRY_AFTER, header::USER_AGENT, Url};
use serde_json::{json, Value};

pub struct ApiAdapter;

impl ApiAdapter {
    pub async fn complete_credentials(
        &self,
        _settings: &AppSettings,
        input: ProviderInput,
        _provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        if !matches!(input.auth.mode, AuthMode::ApiKey) {
            return Err("通用 API 协议只支持 API Key 认证".to_string());
        }

        let key = input.auth.api_key.trim().to_string();
        let key_available = is_full_api_key_value(&key);
        let options = if !key_available {
            Vec::new()
        } else {
            vec![ProviderApiKeyOption::current_for_protocol(
                &key,
                crate::models::ProviderProtocol::Api,
            )]
        };
        Ok(ProviderCredentialCompletionResult {
            input,
            changed_fields: Vec::new(),
            steps: vec![ProviderCredentialCompletionStep {
                name: "API Key".to_string(),
                ok: key_available,
                message: if key.is_empty() {
                    "请填写 API Key；通用协议不提供账号管理或自动创建密钥".to_string()
                } else if !key_available {
                    "请填写完整 API Key，脱敏值不能用于模型调用".to_string()
                } else {
                    "已保留 API Key，可直接调用通用 OpenAI 兼容接口".to_string()
                },
            }],
            api_key_options: options,
        })
    }

    pub async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderConnectionTestResult, String> {
        let client = match build_client(settings, provider).await {
            Ok(client) => client,
            Err(message) => return Ok(connection_failure(message)),
        };
        if !matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Ok(connection_failure(
                "通用 API 协议只支持 API Key 认证".to_string(),
            ));
        }
        if !is_full_api_key_value(&provider.auth.api_key) {
            return Ok(connection_failure("缺少 API Key，无法测试连接".to_string()));
        }

        match fetch_models(&client, provider).await {
            Ok(models) => {
                let mut steps = vec![ProviderConnectionTestStep {
                    name: "模型接口".to_string(),
                    ok: true,
                    message: format!("模型接口返回 {} 个模型", models.len()),
                    available: None,
                    used: None,
                    quota_display: ProviderQuotaDisplay::default(),
                }];
                let mut message =
                    format!("通用 API Key 已连接，模型接口返回 {} 个模型", models.len());
                let (mut available, mut used, mut quota_display) =
                    (None, None, ProviderQuotaDisplay::default());
                let mut ok = true;
                // `/models` 只证明密钥能通过鉴权，余额耗尽或模型被封禁时它仍然会
                // 返回列表。连接测试是用户主动点击的动作，顺手做一次最小真实调用
                // 才能说明这把 Key 现在真的可用。
                if let Some(model) = pick_liveness_model(provider, &models) {
                    match fetch_chat_completion(&client, provider, &model).await {
                        Ok(()) => {
                            message.push_str(&format!("；已用 {model} 完成一次真实调用"));
                            steps.push(ProviderConnectionTestStep {
                                name: "模型调用".to_string(),
                                ok: true,
                                message: format!("已用 {model} 完成一次最小对话请求"),
                                available: None,
                                used: None,
                                quota_display: ProviderQuotaDisplay::default(),
                            });
                        }
                        Err(error) => {
                            ok = false;
                            message.push_str(&format!("；真实调用失败：{}", error.message));
                            steps.push(ProviderConnectionTestStep {
                                name: "模型调用".to_string(),
                                ok: false,
                                message: error.message,
                                available: None,
                                used: None,
                                quota_display: ProviderQuotaDisplay::default(),
                            });
                        }
                    }
                }
                if provider.quota_spec.is_configured() {
                    match fetch_quota(&client, provider).await {
                        Ok(quota) => {
                            message.push_str("；余额接口已读取");
                            if quota.known {
                                available = Some(quota.available);
                                used = quota.total_known.then_some(quota.used);
                            }
                            quota_display = ProviderQuotaDisplay {
                                quota_display_type: quota.display_type.clone(),
                                currency_symbol: quota.currency_symbol.clone(),
                            };
                            steps.push(ProviderConnectionTestStep {
                                name: "余额接口".to_string(),
                                ok: true,
                                message: if quota.unlimited {
                                    "余额接口不限量".to_string()
                                } else if quota.known {
                                    format!(
                                        "可用 {}，{} 个订阅窗口",
                                        quota.available,
                                        quota.windows.len()
                                    )
                                } else {
                                    "余额接口已响应，但没有读到额度字段".to_string()
                                },
                                available: quota.known.then_some(quota.available),
                                used: quota.total_known.then_some(quota.used),
                                quota_display: quota_display.clone(),
                            });
                        }
                        Err(error) => {
                            message.push_str(&format!("；余额接口读取失败：{error}"));
                            steps.push(ProviderConnectionTestStep {
                                name: "余额接口".to_string(),
                                ok: false,
                                message: error.message,
                                available: None,
                                used: None,
                                quota_display: ProviderQuotaDisplay::default(),
                            });
                        }
                    }
                } else {
                    message.push_str("；未声明余额接口");
                }
                Ok(ProviderConnectionTestResult {
                    ok,
                    message,
                    available,
                    used,
                    quota_display,
                    steps,
                })
            }
            Err(error) => Ok(connection_failure(error.message)),
        }
    }

    pub async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        let client = build_client(settings, provider).await?;
        let system_name = host_name(&provider.identity.base_url);
        match fetch_models(&client, provider).await {
            Ok(models) => Ok(ProviderSiteProbeResult {
                ok: true,
                message: format!("通用 API 接口可访问，已读取 {} 个模型", models.len()),
                system_name,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            }),
            Err(error) => Ok(ProviderSiteProbeResult {
                ok: false,
                message: error.message,
                system_name,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            }),
        }
    }

    pub async fn probe_capabilities(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
    ) -> Result<(ProviderCapabilities, String, Option<String>), String> {
        let optional = [
            ProviderOptionalCapability::Pricing,
            ProviderOptionalCapability::Usage,
            ProviderOptionalCapability::RequestLogs,
            ProviderOptionalCapability::Announcements,
            ProviderOptionalCapability::Subscription,
            ProviderOptionalCapability::Groups,
        ]
        .into_iter()
        .map(|capability| {
            ProviderOptionalCapabilityState::unsupported(
                capability,
                "通用 API Key 只提供模型接口，不提供站点级数据接口",
            )
        })
        .collect();
        Ok((
            ProviderCapabilities {
                check_in_known: true,
                check_in_supported: false,
                api_key_management_known: true,
                api_key_management_supported: false,
                invitation_known: true,
                invitation_supported: false,
                optional,
                pricing: ProviderPricingSnapshot::default(),
                ..ProviderCapabilities::default()
            },
            String::new(),
            None,
        ))
    }

    pub async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> (Provider, Option<ProviderFailureKind>) {
        let mut next = provider.clone();
        if !provider.runtime.enabled {
            return (next, None);
        }
        if !matches!(provider.auth.mode, AuthMode::ApiKey) {
            return (
                provider_with_error(&next, "通用 API 协议只支持 API Key 认证".to_string()),
                Some(ProviderFailureKind::Credential),
            );
        }

        let client = match crate::adapters::transport::build_client(settings, provider).await {
            Ok(client) => client,
            Err(message) => {
                return (
                    provider_with_error(&next, message),
                    Some(ProviderFailureKind::Transient),
                )
            }
        };
        match fetch_models(&client, provider).await {
            Ok(models) => {
                next.capabilities
                    .set_available_models(crate::models::ProviderModelList::new(
                        models,
                        crate::models::ProviderModelScope::ApiKey,
                    ));
                next.quota.scope = ProviderQuotaScope::Token;
                let mut failure = None;
                if provider.quota_spec.is_configured() {
                    match fetch_quota(&client, provider).await {
                        Ok(quota) => {
                            next.quota = quota;
                            next.runtime.status = ProviderStatus::Ok;
                            next.runtime.error_message = None;
                        }
                        Err(error) => {
                            // 模型列表已经读到，卡片保留可用状态，只把余额问题写进
                            // 同步提示；旧余额不再可信，一并清空。
                            clear_quota(&mut next);
                            next.runtime.status = ProviderStatus::Warning;
                            next.runtime.error_message =
                                Some(format!("模型列表已同步；{}", error.message));
                            failure = Some(error.failure);
                        }
                    }
                } else {
                    clear_quota(&mut next);
                    next.runtime.status = ProviderStatus::Ok;
                    next.runtime.error_message = None;
                }
                next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                (next, failure)
            }
            Err(error) => {
                next.capabilities.available_models_state.error = Some(error.message.clone());
                let failure = error.failure;
                (provider_with_error(&next, error.message), Some(failure))
            }
        }
    }
}

fn clear_quota(provider: &mut Provider) {
    provider.quota.available = 0.0;
    provider.quota.used = 0.0;
    provider.quota.known = false;
    provider.quota.total_known = false;
    provider.quota.unlimited = false;
    provider.quota.windows.clear();
    provider.quota.scope = ProviderQuotaScope::Token;
}

/// Fetch the OpenAI-compatible model list used by connection checks, refresh,
/// capability probing, and the editor's model picker.
pub async fn fetch_models(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Vec<String>, AdapterError> {
    if provider.auth.api_key.trim().is_empty() {
        return Err(AdapterError::credential("缺少 API Key，无法获取模型列表"));
    }
    let url = models_url(provider)?;
    let request = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    let request = apply_auth_scheme(
        request,
        provider.request.auth_scheme,
        &provider.auth.api_key,
    );
    let response = client.send(request, "读取模型列表").await?;
    let status = response.status;
    let body = response.body;
    if !status.is_success() {
        let payload = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
        if payload.get("type").and_then(Value::as_str) == Some("unauthorized_client_error")
            || payload.pointer("/error/type").and_then(Value::as_str)
                == Some("unauthorized_client_error")
        {
            return Err(AdapterError::credential(format!(
                "站点拒绝此客户端访问模型接口（HTTP {}）",
                status.as_u16()
            )));
        }
        let detail = body.chars().take(240).collect::<String>();
        return Err(AdapterError::from_status(
            format!("获取模型列表失败: HTTP {status} {detail}"),
            status,
            response.headers.get(RETRY_AFTER),
        ));
    }

    parse_models(&body).map_err(AdapterError::transient)
}

fn parse_models(body: &str) -> Result<Vec<String>, String> {
    let value =
        serde_json::from_str::<Value>(body).map_err(|err| format!("解析模型列表失败: {err}"))?;
    if value.get("success").and_then(Value::as_bool) == Some(false)
        || value.get("error").is_some_and(|error| !error.is_null())
    {
        return Err("站点返回模型列表错误，已保留上次结果".to_string());
    }
    parse_model_data(&value)
}

pub(super) fn parse_model_data(value: &Value) -> Result<Vec<String>, String> {
    let values = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .ok_or_else(|| "模型列表响应缺少 data 数组".to_string())?;
    let mut models = values
        .iter()
        .filter_map(|item| {
            item.as_str()
                .map(str::to_string)
                .or_else(|| item.get("id").and_then(Value::as_str).map(str::to_string))
                .or_else(|| {
                    item.get("model")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| item.get("name").and_then(Value::as_str).map(str::to_string))
        })
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    limits::truncate_models(&mut models);
    if !values.is_empty() && models.is_empty() {
        return Err("模型列表内容无效，已保留上次结果".to_string());
    }
    Ok(models)
}

fn models_url(provider: &Provider) -> Result<Url, String> {
    let raw = provider.identity.base_url.trim();
    if raw.is_empty() {
        return Err("缺少模型 Base URL 或中转站地址".to_string());
    }
    let declared = provider.model_config.models_url.trim();
    if !declared.is_empty() {
        let endpoint = resolve_provider_url(raw, declared)?;
        return Url::parse(&endpoint).map_err(|err| format!("模型列表地址无效: {err}"));
    }
    let normalized = raw.trim_end_matches('/');
    let endpoint = if normalized.ends_with("/models") {
        normalized.to_string()
    } else if normalized.ends_with("/v1") {
        format!("{normalized}/models")
    } else {
        format!("{normalized}/v1/models")
    };
    Url::parse(&endpoint).map_err(|err| format!("模型列表地址无效: {err}"))
}

fn chat_completions_url(provider: &Provider) -> Result<Url, String> {
    let raw = provider.identity.base_url.trim();
    if raw.is_empty() {
        return Err("缺少模型 Base URL 或中转站地址".to_string());
    }
    let normalized = raw.trim_end_matches('/');
    if normalized.ends_with("/chat/completions") {
        return Url::parse(normalized).map_err(|err| format!("模型调用地址无效: {err}"));
    }
    let last_segment = normalized.rsplit('/').next().unwrap_or_default();
    // 已经带版本段的网关不再重复追加 `/v1`；OpenRouter、Fireworks 这类
    // 地址的版本段前面还有自己的路径前缀。
    let endpoint = if is_version_segment(last_segment) {
        format!("{normalized}/chat/completions")
    } else {
        format!("{normalized}/v1/chat/completions")
    };
    Url::parse(&endpoint).map_err(|err| format!("模型调用地址无效: {err}"))
}

fn is_version_segment(segment: &str) -> bool {
    segment.len() >= 2
        && segment.starts_with('v')
        && segment[1..]
            .chars()
            .all(|character| character.is_ascii_digit())
}

/// 连接测试里的真实调用优先使用卡片自己配置的测活模型。没有配置时从模型
/// 列表里挑一个至少不是 embedding、语音或图片专用接口的模型，避免拿一个
/// 必然不支持对话的模型去误判整张卡片。
fn pick_liveness_model(provider: &Provider, models: &[String]) -> Option<String> {
    let configured = provider.liveness.model.trim();
    if !configured.is_empty() {
        return Some(configured.to_string());
    }
    models
        .iter()
        .find(|model| is_chat_capable_model(model))
        .cloned()
}

fn is_chat_capable_model(model: &str) -> bool {
    const NON_CHAT_MARKERS: &[&str] = &[
        "embed",
        "rerank",
        "whisper",
        "tts",
        "moderation",
        "dall-e",
        "stable-diffusion",
        "flux",
        "image",
        "audio",
        "speech",
        "transcribe",
        "ocr",
    ];
    let normalized = model.to_ascii_lowercase();
    !NON_CHAT_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
}

async fn fetch_chat_completion(
    client: &ProviderTransport,
    provider: &Provider,
    model: &str,
) -> Result<(), AdapterError> {
    let url = chat_completions_url(provider)?;
    let request = client
        .post(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json")
        .json(&json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Reply with OK." }],
            "stream": false,
        }));
    let request = apply_auth_scheme(
        request,
        provider.request.auth_scheme,
        &provider.auth.api_key,
    );
    let response = client.send(request, "真实模型调用").await?;
    let status = response.status;
    let body = response.body;
    if !status.is_success() {
        let detail = body.chars().take(240).collect::<String>();
        return Err(AdapterError::from_status(
            format!("真实模型调用失败: HTTP {status} {detail}"),
            status,
            response.headers.get(RETRY_AFTER),
        ));
    }
    let value = serde_json::from_str::<Value>(&body)
        .map_err(|err| AdapterError::transient(format!("解析模型调用响应失败: {err}")))?;
    if value.get("error").is_some_and(|error| !error.is_null()) {
        return Err(AdapterError::transient(
            "模型调用返回错误，响应里没有可用结果".to_string(),
        ));
    }
    if !value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| !choices.is_empty())
    {
        return Err(AdapterError::transient(
            "模型调用已返回，但响应里没有 choices".to_string(),
        ));
    }
    Ok(())
}

fn host_name(base_url: &str) -> Option<String> {
    Url::parse(base_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
}

fn provider_with_error(provider: &Provider, message: String) -> Provider {
    let mut next = provider.clone();
    next.runtime.status = ProviderStatus::Error;
    next.runtime.error_message = Some(message);
    next.quota.scope = ProviderQuotaScope::Token;
    next.quota.known = false;
    next.quota.total_known = false;
    next
}

fn connection_failure(message: String) -> ProviderConnectionTestResult {
    ProviderConnectionTestResult {
        ok: false,
        message: message.clone(),
        available: None,
        used: None,
        quota_display: ProviderQuotaDisplay::default(),
        steps: vec![ProviderConnectionTestStep {
            name: "模型接口".to_string(),
            ok: false,
            message,
            available: None,
            used: None,
            quota_display: ProviderQuotaDisplay::default(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::test_support::{serve, Reply};
    use crate::models::{
        AgentCliKind, AppSettings, ProviderIdentityInput, ProviderInput, ProviderProtocol,
    };

    fn provider() -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    base_url: "https://relay.example.com/openai".to_string(),
                    protocol: ProviderProtocol::Api,
                    ..ProviderIdentityInput::default()
                },
                auth: crate::models::ProviderAuth {
                    mode: AuthMode::ApiKey,
                    api_key: "sk-test".to_string(),
                    ..ProviderInput::default().auth
                },
                ..ProviderInput::default()
            },
            "generic-test".to_string(),
        )
    }

    #[test]
    fn appends_v1_models_to_a_gateway_base_url() {
        assert_eq!(
            models_url(&provider()).unwrap().as_str(),
            "https://relay.example.com/openai/v1/models"
        );
    }

    #[test]
    fn keeps_an_explicit_v1_models_url() {
        let mut provider = provider();
        provider.identity.base_url = "https://relay.example.com/v1/models".to_string();
        assert_eq!(
            models_url(&provider).unwrap().as_str(),
            "https://relay.example.com/v1/models"
        );
    }

    #[test]
    fn ignores_agent_specific_model_endpoint_overrides() {
        let mut provider = provider();
        provider.liveness.agent_base_urls.insert(
            AgentCliKind::Codex,
            "https://relay.example.com/override/v1/models".to_string(),
        );
        assert_eq!(
            models_url(&provider).unwrap().as_str(),
            "https://relay.example.com/openai/v1/models"
        );
    }

    #[test]
    fn parses_object_and_string_model_entries() {
        let models =
            parse_models(r#"{"data":[{"id":"gpt-4o"},{"model":"claude-3"},"custom"]}"#).unwrap();
        assert_eq!(models, vec!["claude-3", "custom", "gpt-4o"]);
    }

    #[test]
    fn empty_model_list_is_a_successful_snapshot() {
        assert_eq!(
            parse_models(r#"{"data":[]}"#).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(parse_models("[]").unwrap(), Vec::<String>::new());
        assert!(parse_models(r#"{"success":false,"data":[]}"#).is_err());
        assert!(parse_models(r#"{"data":[{}]}"#).is_err());
        assert!(parse_models("<html>challenge</html>").is_err());
    }

    #[test]
    fn chat_url_appends_v1_to_a_bare_gateway_address() {
        assert_eq!(
            chat_completions_url(&provider()).unwrap().as_str(),
            "https://relay.example.com/openai/v1/chat/completions"
        );
    }

    #[test]
    fn chat_url_keeps_an_existing_version_segment() {
        let mut provider = provider();
        provider.identity.base_url = "https://open.bigmodel.cn/api/paas/v4".to_string();
        assert_eq!(
            chat_completions_url(&provider).unwrap().as_str(),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn chat_url_keeps_an_explicit_endpoint() {
        let mut provider = provider();
        provider.identity.base_url = "https://relay.example.com/v1/chat/completions".to_string();
        assert_eq!(
            chat_completions_url(&provider).unwrap().as_str(),
            "https://relay.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn configured_liveness_model_wins_and_embedding_models_are_skipped() {
        let mut provider = provider();
        provider.liveness.model = String::new();
        let models = vec![
            "text-embedding-3-small".to_string(),
            "gpt-4o-mini".to_string(),
        ];
        assert_eq!(
            pick_liveness_model(&provider, &models),
            Some("gpt-4o-mini".to_string())
        );
        assert_eq!(
            pick_liveness_model(&provider, &["text-embedding-3-large".to_string()]),
            None
        );
        provider.liveness.model = "custom-model".to_string();
        assert_eq!(
            pick_liveness_model(&provider, &models),
            Some("custom-model".to_string())
        );
    }

    #[tokio::test]
    async fn chat_liveness_posts_a_minimal_non_streaming_request() {
        let (provider, requests, server) = serve(vec![Reply(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"OK"}}]}"#,
        )]);
        let client = build_client(&AppSettings::default(), &provider)
            .await
            .unwrap();
        fetch_chat_completion(&client, &provider, "gpt-4o-mini")
            .await
            .unwrap();
        server.join().unwrap();
        let request = requests.recv().unwrap();
        assert!(request.starts_with("POST /relay/v1/chat/completions "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-fixture-model-key"));
        assert!(request.contains("\"model\":\"gpt-4o-mini\""));
        assert!(request.contains("\"stream\":false"));
    }

    #[tokio::test]
    async fn chat_liveness_surfaces_an_http_rejection() {
        let (provider, _requests, server) =
            serve(vec![Reply(401, r#"{"error":{"message":"invalid key"}}"#)]);
        let client = build_client(&AppSettings::default(), &provider)
            .await
            .unwrap();
        let error = fetch_chat_completion(&client, &provider, "gpt-4o-mini")
            .await
            .unwrap_err();
        server.join().unwrap();
        assert!(error.message.contains("HTTP 401"));
    }
}
