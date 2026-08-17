//! Adapter for OpenAI-compatible gateways whose account protocol is unknown.
//!
//! A generic gateway is intentionally key-scoped. It can be used for model
//! calls and model discovery, but it must never be treated as a NewAPI or
//! Sub2API account endpoint merely because the user supplied an API key.

mod protocol;

use crate::{
    adapters::transport::{build_client, ProviderTransport, USER_AGENT_VALUE},
    limits,
    models::{
        AppSettings, AuthMode, Provider, ProviderApiKeyOption, ProviderCapabilities,
        ProviderConnectionTestResult, ProviderConnectionTestStep,
        ProviderCredentialCompletionResult, ProviderCredentialCompletionStep, ProviderInput,
        ProviderQuotaDisplay, ProviderQuotaScope, ProviderSiteProbeResult, ProviderStatus,
    },
};
use reqwest::{header::ACCEPT, header::USER_AGENT, Url};
use serde_json::Value;

pub(crate) struct ApiAdapter;

impl ApiAdapter {
    pub(crate) async fn complete_credentials(
        &self,
        _settings: &AppSettings,
        input: ProviderInput,
        _provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        if !matches!(input.auth.mode, AuthMode::ApiKey) {
            return Err("通用 API 协议只支持 API Key 认证".to_string());
        }

        let key = input.auth.api_key.trim().to_string();
        let options = if key.is_empty() {
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
                ok: !key.is_empty(),
                message: if key.is_empty() {
                    "请填写 API Key；通用协议不提供账号管理或自动创建密钥".to_string()
                } else {
                    "已保留 API Key，可直接调用通用 OpenAI 兼容接口".to_string()
                },
            }],
            api_key_options: options,
        })
    }

    pub(crate) async fn test_connection(
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
        if provider.auth.api_key.trim().is_empty() {
            return Ok(connection_failure("缺少 API Key，无法测试连接".to_string()));
        }

        match fetch_models(&client, provider).await {
            Ok(models) => {
                let message = format!(
                    "通用 API Key 已连接，模型接口返回 {} 个模型；账号额度不可用",
                    models.len()
                );
                let step = ProviderConnectionTestStep {
                    name: "模型接口".to_string(),
                    ok: true,
                    message: message.clone(),
                    available: None,
                    used: None,
                    quota_display: ProviderQuotaDisplay::default(),
                };
                Ok(ProviderConnectionTestResult {
                    ok: true,
                    message,
                    available: None,
                    used: None,
                    quota_display: ProviderQuotaDisplay::default(),
                    steps: vec![step],
                })
            }
            Err(message) => Ok(connection_failure(message)),
        }
    }

    pub(crate) async fn probe_site(
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
            Err(message) => Ok(ProviderSiteProbeResult {
                ok: false,
                message,
                system_name,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            }),
        }
    }

    pub(crate) async fn probe_capabilities(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
    ) -> Result<(ProviderCapabilities, String, Option<String>), String> {
        Ok((
            ProviderCapabilities {
                check_in_known: true,
                check_in_supported: false,
                api_key_management_known: true,
                api_key_management_supported: false,
                invitation_known: true,
                invitation_supported: false,
                ..ProviderCapabilities::default()
            },
            String::new(),
            None,
        ))
    }

    pub(crate) async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Provider {
        let mut next = provider.clone();
        if !provider.runtime.enabled {
            return next;
        }
        if !matches!(provider.auth.mode, AuthMode::ApiKey) {
            return provider_with_error(&next, "通用 API 协议只支持 API Key 认证".to_string());
        }

        let client = match crate::adapters::transport::build_client(settings, provider).await {
            Ok(client) => client,
            Err(message) => return provider_with_error(&next, message),
        };
        match fetch_models(&client, provider).await {
            Ok(models) => {
                next.quota.available = 0.0;
                next.quota.used = 0.0;
                next.quota.known = false;
                next.quota.total_known = false;
                next.quota.unlimited = false;
                next.quota.scope = ProviderQuotaScope::Token;
                next.capabilities.available_models = models;
                next.runtime.status = ProviderStatus::Ok;
                next.runtime.error_message = None;
                next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
                next
            }
            Err(message) => provider_with_error(&next, message),
        }
    }
}

/// Fetch the OpenAI-compatible model list used by connection checks, refresh,
/// capability probing, and the editor's model picker.
pub(crate) async fn fetch_models(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Vec<String>, String> {
    if provider.auth.api_key.trim().is_empty() {
        return Err("缺少 API Key，无法获取模型列表".to_string());
    }
    let url = models_url(provider)?;
    let request = client
        .get(url)
        .bearer_auth(provider.auth.api_key.trim())
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json");
    let response = client.send(request, "读取模型列表").await?;
    let status = response.status;
    let body = response.body;
    if !status.is_success() {
        let detail = body.chars().take(240).collect::<String>();
        return Err(format!("获取模型列表失败: HTTP {status} {detail}"));
    }

    parse_models(&body)
}

fn parse_models(body: &str) -> Result<Vec<String>, String> {
    let value =
        serde_json::from_str::<Value>(body).map_err(|err| format!("解析模型列表失败: {err}"))?;
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
    if models.is_empty() {
        return Err("模型列表为空".to_string());
    }
    Ok(models)
}

fn models_url(provider: &Provider) -> Result<Url, String> {
    let raw = provider.identity.base_url.trim();
    if raw.is_empty() {
        return Err("缺少模型 Base URL 或中转站地址".to_string());
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
    use crate::models::{AgentCliKind, ProviderIdentityInput, ProviderInput, ProviderProtocol};

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
}
