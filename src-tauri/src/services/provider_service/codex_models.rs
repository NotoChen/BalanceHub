use crate::{
    adapters::{api, transport::build_client},
    models::{provider_domain, AppSettings, Provider},
};

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
    let client = build_client(settings, provider).await?;
    api::fetch_models(&client, provider).await
}
