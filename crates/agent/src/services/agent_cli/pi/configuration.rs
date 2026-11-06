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
                label: "Pi 用户设置",
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
            "全局 settings.json；项目 .pi/settings.json 仍按原生优先级覆盖",
        ))
        .is_break()
    {
        return;
    }
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "models",
            label: "Pi 模型与服务配置",
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
        "全局 models.json；定义中转站 Provider、模型列表与覆盖规则",
    ));
}
