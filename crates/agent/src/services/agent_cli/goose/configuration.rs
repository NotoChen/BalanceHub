use super::config;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
        environment::SourceInput,
    },
};
use std::path::Path;

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
    let root = Path::new(&request.context.config_root);
    if output
        .emit(support::document(
            SourceInput {
                origin: AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "config",
                label: "Goose 配置",
                path: config::config_path(root),
                allowed_root: root,
                scope: AgentAssetScope::User,
                precedence: 10,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[],
            },
            AgentConfigurationFormat::Yaml,
            true,
            "config.yaml；active_provider 指向 providers.balancehub，旧版同时读取 GOOSE_PROVIDER 与 GOOSE_MODEL",
        ))
        .is_break()
    {
        return;
    }
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "custom-provider",
            label: "Goose 自定义 Provider",
            path: config::provider_path(root),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Json,
        true,
        "custom_providers/balancehub.json；地址、Authorization 头与模型列表由中转站切换写入",
    ));
}
