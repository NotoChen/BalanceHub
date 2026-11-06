use super::config;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
        environment::SourceInput,
    },
};

pub(super) const fn adapter() -> AgentConfigurationAdapter {
    AgentConfigurationAdapter {
        discover,
        validate: support::validate,
    }
}

fn discover(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn AgentConfigurationSourceOutput,
) {
    let path = config::config_path_for_home(request.home);
    let root = path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| request.home.to_path_buf());
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "config",
            label: "Mister Morph 配置",
            path,
            allowed_root: &root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Yaml,
        true,
        "config.yaml；llm.inference_provider 为 openai_response_compatible，模型走 Responses API",
    ));
}
