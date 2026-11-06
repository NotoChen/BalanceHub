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
    let root = config::config_dir_for_home(request.home);
    let path = config::settings_path(&root);
    let allowed_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| request.home.to_path_buf());
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "settings",
            label: "Qoder 设置",
            path,
            allowed_root: &allowed_root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Jsonc,
        true,
        "全局 settings.json；providers 保存自定义 Provider，model.name 以 <provider>/<模型> 选择模型",
    ));
}
