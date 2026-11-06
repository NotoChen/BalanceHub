use super::config;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
        environment::SourceInput,
    },
};
use std::path::{Path, PathBuf};

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
    let settings = config::settings_path(&root);
    let settings_root = parent_or_home(&settings, request.home);
    if output
        .emit(source(
            &settings_root,
            settings,
            "settings",
            "Copilot CLI 设置",
            "全局 settings.json；model 写为 balancehub/<模型 ID>",
        ))
        .is_break()
    {
        return;
    }
    let providers = config::providers_path(&root);
    let providers_root = parent_or_home(&providers, request.home);
    let _ = output.emit(source(
        &providers_root,
        providers,
        "providers",
        "Copilot CLI BYOK Provider 配置",
        "providers.json 为 Copilot CLI 提供自定义 Provider 与模型，不需要 GitHub 登录",
    ));
}

fn parent_or_home(path: &Path, home: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.to_path_buf())
}

fn source(
    allowed_root: &Path,
    path: PathBuf,
    key: &'static str,
    label: &'static str,
    load_rule: &'static str,
) -> AgentConfigurationSourceSpec {
    support::document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: key,
            label,
            path,
            allowed_root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Jsonc,
        true,
        load_rule,
    )
}
