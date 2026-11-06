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
                native_source_key: "patch",
                label: "dsh 配置补丁",
                path: config::patch_path_for_root(root),
                allowed_root: root,
                scope: AgentAssetScope::User,
                precedence: 10,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[],
            },
            AgentConfigurationFormat::Yaml,
            true,
            "cordis.patch.yml；llm-pi-ai 保存中转站地址与 Key，agent-default-model 保存当前模型",
        ))
        .is_break()
    {
        return;
    }
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "settings",
            label: "dsh 设置",
            path: config::settings_path_for_root(root),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Yaml,
        false,
        "settings.yaml；ACP 与桌面端读取的 llm-pi-ai 路线和已选模型",
    ));
}
