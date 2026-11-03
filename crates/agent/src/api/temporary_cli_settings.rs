//! 临时 CLI 启动能力与启动方案命令。

use crate::{
    api::blocking,
    models::*,
    services::{provider_service::ProviderService, temporary_cli},
};

pub(crate) async fn get_agent_model_catalog(
    context: AgentModelCatalogContext,
    refresh: bool,
) -> Result<AgentModelCatalog, String> {
    let settings = crate::state::app_data().settings;
    blocking("读取 Agent 模型目录", move || {
        crate::services::agent_cli::settings::read_model_catalog(&context, &settings, refresh)
    })
    .await
}

pub(crate) async fn get_temporary_cli_settings(
    request: TemporaryCliSettingsRequest,
) -> Result<TemporaryCliSettingsCapabilities, String> {
    // 卡片声明的模型推理能力以 Rust 侧数据为准；前端只传本次启动的中转站。
    let model_config = crate::state::app_data()
        .providers
        .iter()
        .find(|provider| provider.identity.id == request.provider_id)
        .map(|provider| provider.model_config.clone());
    blocking("读取本次启动能力", move || {
        temporary_cli::run_capabilities(&request, model_config.as_ref())
    })
    .await
}

pub(crate) async fn get_temporary_cli_assets(
    request: TemporaryCliSettingsRequest,
    refresh: bool,
) -> Result<TemporaryCliAssetListing, String> {
    let settings = crate::state::app_data().settings;
    blocking("读取本次启动资产", move || {
        temporary_cli::run_asset_options(&settings, &request, refresh)
    })
    .await
}

pub(crate) fn list_temporary_cli_profiles() -> Vec<TemporaryCliProfile> {
    crate::state::app_data().temporary_cli_profiles
}

pub(crate) async fn save_temporary_cli_profile(
    profile: TemporaryCliProfile,
) -> Result<Vec<TemporaryCliProfile>, String> {
    blocking("保存启动方案", move || {
        ProviderService::new().save_temporary_cli_profile(profile)
    })
    .await
}

pub(crate) async fn delete_temporary_cli_profile(
    id: String,
) -> Result<Vec<TemporaryCliProfile>, String> {
    blocking("删除启动方案", move || {
        ProviderService::new().delete_temporary_cli_profile(&id)
    })
    .await
}
