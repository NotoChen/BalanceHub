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
                native_source_key: "settings",
                label: "OmO 用户设置",
                path: config::settings_path(root),
                allowed_root: root,
                scope: AgentAssetScope::User,
                precedence: 10,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[],
            },
            AgentConfigurationFormat::Jsonc,
            true,
            "settings.json；OmO 沿用 Pi 的 defaultProvider 与 defaultModel 字段",
        ))
        .is_break()
    {
        return;
    }
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "models",
            label: "OmO 模型与服务配置",
            path: config::models_path(root),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Jsonc,
        true,
        "models.json；providers.balancehub 提供 OpenAI Chat Completions 地址、Key 与模型列表",
    ));
}
