use crate::{
    adapters::{
        api,
        transport::{build_client, USER_AGENT_VALUE},
    },
    models::{provider_domain, AppSettings, Provider, ProviderProtocol},
    services::liveness::openai_base_url,
};
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct OpenAiModelList {
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
}

pub(super) async fn fetch_codex_models(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<Vec<String>, String> {
    if !provider_domain::auth::has_api_key(provider) {
        return Err("缺少 API Key，无法获取模型列表".to_string());
    }
    if provider.liveness.openai_base_url.trim().is_empty()
        && provider.identity.base_url.trim().is_empty()
    {
        return Err("缺少模型 Base URL 或中转站地址".to_string());
    }

    if matches!(provider.identity.protocol, ProviderProtocol::Api) {
        let client = build_client(settings, provider)?;
        return api::fetch_models(&client, provider).await;
    }

    let mut base_url = openai_base_url(provider);
    if matches!(provider.identity.protocol, ProviderProtocol::Sub2Api)
        && !base_url.trim_end_matches('/').ends_with("/v1")
    {
        base_url = format!("{}/v1", base_url.trim_end_matches('/'));
    }
    let url = reqwest::Url::parse(&format!("{}/models", base_url.trim_end_matches('/')))
        .map_err(|err| format!("模型列表地址无效: {err}"))?;
    let client = build_client(settings, provider)?;
    let response = client
        .get(url)
        .bearer_auth(provider.auth.api_key.trim())
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|err| format!("获取模型列表失败: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("读取模型列表失败: {err}"))?;
    if !status.is_success() {
        let detail = body.chars().take(240).collect::<String>();
        return Err(format!("获取模型列表失败: HTTP {status} {detail}"));
    }

    let mut models = serde_json::from_str::<OpenAiModelList>(&body)
        .map_err(|err| format!("解析模型列表失败: {err}"))?
        .data
        .into_iter()
        .map(|model| model.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err("模型列表为空".to_string());
    }
    Ok(models)
}
