use super::environment;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
        environment::config_document::ConfigDocumentFormat,
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
    for native in support::collect_native_sources(
        environment::discover_resource_sources,
        request,
        &[
            "config",
            "workspace-config",
            "system-config",
            "system-requirements",
            "auth",
        ],
    ) {
        let role = native.native_source_key.clone();
        let may_create = !matches!(
            native.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        );
        let auth = role == "auth";
        let spec = support::describe(
            native,
            if auth {
                AgentConfigurationFormat::Json
            } else {
                AgentConfigurationFormat::Toml
            },
            may_create,
            "候选来源按 Codex 原生作用域、项目信任和启动参数决定是否加载",
            "保存后，请在新 Codex 会话中确认配置",
        );
        if output.emit(spec).is_break() {
            return;
        }
    }
    discover_additional(request, output);
}

pub(super) fn discover_additional(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn AgentConfigurationSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    for path in support::regular_files(root, root, ".config.toml") {
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".config.toml"))
        else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let name = name.to_owned();
        let mut spec = support::document(
            crate::services::agent_cli::environment::SourceInput {
                origin: AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: &format!("profile:{name}"),
                label: "Codex Profile",
                path,
                allowed_root: root,
                scope: AgentAssetScope::User,
                precedence: 15,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[],
            },
            AgentConfigurationFormat::Toml,
            false,
            "只有适用版本显式选择该 profile 才参与加载；未检查运行会话",
        );
        spec.profile = Some(name);
        spec.version_requirement = Some(
            "独立 *.config.toml profile 需要支持该格式的 Codex 版本；不迁移旧 profiles 表".into(),
        );
        if output.emit(spec).is_break() {
            return;
        }
    }
    let mut filenames = vec!["AGENTS.override.md".to_owned(), "AGENTS.md".to_owned()];
    filenames.extend(support::configured_filenames(
        root,
        &root.join("config.toml"),
        ConfigDocumentFormat::Toml,
        &["project_doc_fallback_filenames"],
    ));
    filenames.dedup();
    for name in &filenames {
        if output
            .emit(support::instruction(
                root,
                root.join(name),
                &format!("user-instructions:{name}"),
                "Codex 用户指令",
                AgentAssetScope::User,
                matches!(name.as_str(), "AGENTS.md" | "AGENTS.override.md"),
                "按原生候选顺序选择；AGENTS.override.md 优先于 AGENTS.md，启动新会话时加载",
            ))
            .is_break()
        {
            return;
        }
    }
    if let Some(workspace) = request.workspace {
        for (index, directory) in support::repository_chain(workspace, output)
            .into_iter()
            .enumerate()
        {
            if directory.parent().is_none() {
                continue;
            }
            for name in &filenames {
                if output
                    .emit(support::instruction(
                        &directory,
                        directory.join(name),
                        &format!("workspace-instructions:{index}:{name}"),
                        "Codex 项目指令",
                        AgentAssetScope::Workspace,
                        directory == workspace
                            && matches!(name.as_str(), "AGENTS.md" | "AGENTS.override.md"),
                        "项目根至当前目录逐层加载；每层只选第一个适用文件，信任与运行参数尚未验证",
                    ))
                    .is_break()
                {
                    return;
                }
            }
            if directory != workspace {
                let spec = support::document(
                    crate::services::agent_cli::environment::SourceInput {
                        origin: AgentAssetInstallationOrigin::ConfigEntry,
                        native_source_key: &format!("ancestor-config:{index}"),
                        label: "Codex 上层项目配置",
                        path: directory.join(".codex/config.toml"),
                        allowed_root: &directory,
                        scope: AgentAssetScope::Workspace,
                        precedence: 20,
                        sensitive: true,
                        source_kind: AgentAssetSourceKind::File,
                        categories: &[],
                    },
                    AgentConfigurationFormat::Toml,
                    false,
                    "适用项目信任与原生分层规则；未证明当前会话加载",
                );
                if output.emit(spec).is_break() {
                    return;
                }
            }
        }
    }
}
