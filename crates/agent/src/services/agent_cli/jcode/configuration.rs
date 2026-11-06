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
    let path = config::config_path(&root);
    let allowed_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| request.home.to_path_buf());
    let _ = output.emit(support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "settings",
            label: "jcode 配置",
            path,
            allowed_root: &allowed_root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Toml,
        true,
        "全局 config.toml；[provider] 选择默认 Provider 与模型，[providers.balancehub] 保存中转站地址、密钥和模型条目",
    ));
}
