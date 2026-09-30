use super::environment;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
        environment::{config_document::ConfigDocumentFormat, SourceInput},
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
            "settings",
            "workspace-settings",
            "system-defaults",
            "system-settings",
            "trusted-folders",
        ],
    ) {
        let may_create = !matches!(
            native.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) && native.native_source_key != "trusted-folders";
        let spec = support::describe(
            native,
            AgentConfigurationFormat::Jsonc,
            may_create,
            "用户与项目设置按原生分层规则加载，系统覆盖和运行环境可能改变结果",
            "保存后，部分 Gemini 设置需要新会话或原生重载",
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
    if output
        .emit(support::credential_cache(
            root,
            root.join("oauth_creds.json"),
            "oauth-cache",
            "Gemini OAuth 缓存",
        ))
        .is_break()
    {
        return;
    }
    if output
        .emit(support::credential_cache(
            root,
            root.join("google_accounts.json"),
            "account-cache",
            "Gemini 账号缓存",
        ))
        .is_break()
    {
        return;
    }
    if discover_environment_files(request, output).is_break() {
        return;
    }
    let mut filenames = support::configured_filenames(
        root,
        &root.join("settings.json"),
        ConfigDocumentFormat::Jsonc,
        &["context", "fileName"],
    );
    if let Some(workspace) = request.workspace {
        let names = support::configured_filenames(
            workspace,
            &workspace.join(".gemini/settings.json"),
            ConfigDocumentFormat::Jsonc,
            &["context", "fileName"],
        );
        if !names.is_empty() {
            filenames = names;
        }
    }
    if filenames.is_empty() {
        filenames.push("GEMINI.md".into());
    }
    filenames.sort();
    filenames.dedup();
    for name in &filenames {
        if output
            .emit(support::instruction(
                root,
                root.join(name),
                &format!("user-instructions:{name}"),
                "Gemini CLI 用户指令",
                AgentAssetScope::User,
                name == "GEMINI.md",
                "指令名称来自 context.fileName 或原生默认 GEMINI.md；加载链需在原生会话确认",
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
                        "Gemini CLI 项目指令",
                        AgentAssetScope::Workspace,
                        directory == workspace && name == "GEMINI.md",
                        "项目边界至所选工作区的指令候选；项目信任、自定义边界与子目录按需加载尚未验证",
                    ))
                    .is_break()
                {
                    return;
                }
            }
        }
    }
}

fn discover_environment_files(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn AgentConfigurationSourceOutput,
) -> std::ops::ControlFlow<()> {
    let root = Path::new(&request.context.config_root);
    let mut locations = vec![(
        root.to_owned(),
        root.to_owned(),
        "environment".to_owned(),
        AgentAssetScope::User,
        true,
    )];
    if request.home.join(".env").exists() {
        locations.push((
            request.home.to_owned(),
            request.home.to_owned(),
            "environment-home".to_owned(),
            AgentAssetScope::User,
            false,
        ));
    }
    if let Some(workspace) = request.workspace {
        // Current upstream checks .gemini/.env then .env at every ancestor.
        // Documentation/version/trust differ, so publish candidates, not a winner.
        for (index, directory) in support::ancestor_chain(workspace, None, output)
            .into_iter()
            .rev()
            .enumerate()
        {
            for (suffix, native_dir) in [
                ("gemini", directory.join(".gemini")),
                ("plain", directory.clone()),
            ] {
                let path = native_dir.join(".env");
                if directory != workspace && std::fs::symlink_metadata(&path).is_err() {
                    continue;
                }
                let key = if directory == workspace {
                    if suffix == "gemini" {
                        "workspace-environment-gemini".to_owned()
                    } else {
                        "workspace-environment".to_owned()
                    }
                } else {
                    format!("ancestor-environment:{index}:{suffix}")
                };
                let may_create = directory == workspace;
                locations.push((
                    native_dir,
                    directory.clone(),
                    key,
                    AgentAssetScope::Workspace,
                    may_create,
                ));
            }
        }
    }
    let mut emitted = std::collections::BTreeSet::new();
    for (directory, allowed_root, key, scope, may_create) in locations {
        let path = directory.join(".env");
        if !emitted.insert(path.clone()) {
            continue;
        }
        let spec = support::document(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry, native_source_key: &key, label: "Gemini CLI 环境变量文件", path,
            allowed_root: &allowed_root, scope, precedence: if scope == AgentAssetScope::User { 10 } else { 20 },
            sensitive: true, source_kind: AgentAssetSourceKind::File, categories: &[],
        }, AgentConfigurationFormat::Dotenv, may_create,
            "仅列原生候选来源；版本、项目信任、忽略 .env 选项和继承环境决定实际加载，不表示这些文件同时合并");
        if output.emit(spec).is_break() {
            return std::ops::ControlFlow::Break(());
        }
    }
    std::ops::ControlFlow::Continue(())
}
