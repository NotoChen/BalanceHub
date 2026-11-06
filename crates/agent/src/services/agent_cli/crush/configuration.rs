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
    let config_path = config::config_path_for_home(request.home);
    let data_path = config::data_path_for_home(request.home);
    let config_root = parent_or_home(&config_path, request.home);
    if output
        .emit(source(
            &config_root,
            config_path.clone(),
            "settings",
            "Crush 配置",
            "Crush 读取其中的 providers 与模型列表；数据目录中的同名文件会覆盖模型选择",
        ))
        .is_break()
    {
        return;
    }
    if data_path != config_path {
        let data_root = parent_or_home(&data_path, request.home);
        let _ = output.emit(source(
            &data_root,
            data_path,
            "crush-data",
            "Crush 模型选择",
            "Crush 自己的模型选择器把 large / small 的选择和单模型参数保存在这里，并在 crush.json 之后读取",
        ));
    }
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
