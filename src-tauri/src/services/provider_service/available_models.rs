use crate::{
    adapters::{api, transport::build_client},
    models::{provider_domain, AppSettings, Provider},
};

pub(super) async fn fetch_available_models(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<Vec<String>, String> {
    if !provider_domain::auth::has_api_key(provider) {
        return Err("缺少 API Key，无法获取模型列表".to_string());
    }
    let client = build_client(settings, provider).await?;
    api::fetch_models(&client, provider).await
}
