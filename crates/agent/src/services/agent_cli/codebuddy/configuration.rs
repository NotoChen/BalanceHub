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
            "CodeBuddy 设置",
            "全局 settings.json；model 从 models.json 的模型列表中选择",
        ))
        .is_break()
    {
        return;
    }
    let models = config::models_path(&root);
    let models_root = parent_or_home(&models, request.home);
    let _ = output.emit(source(
        &models_root,
        models,
        "models",
        "CodeBuddy 模型与服务配置",
        "models.json 定义自定义模型；URL 指向 OpenAI Chat Completions 端点",
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
