use super::environment;
mod local;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
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
            "workspace-local-settings",
            "workspace-mcp",
            "managed-settings",
            "managed-mcp",
            "account",
        ],
    ) {
        let may_create = !matches!(
            native.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) && native.native_source_key != "account";
        let spec = support::describe(
            native,
            AgentConfigurationFormat::Json,
            may_create,
            "按用户、项目共享、项目个人和管理员来源区分；工作区/运行参数可能改变加载结果",
            "保存后，模型等设置需要在原生命令或新会话中确认",
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
    if let Some(spec) = local::candidate(request, output) {
        if output.emit(spec).is_break() {
            return;
        }
    }
    let root = Path::new(&request.context.config_root);
    let user_instructions = root.join("CLAUDE.md");
    if output
        .emit(support::credential_cache(
            root,
            root.join(".credentials.json"),
            "credentials-cache",
            "Claude 原生凭据缓存",
        ))
        .is_break()
    {
        return;
    }
    if output
        .emit(support::instruction(
            root,
            user_instructions.clone(),
            "user-instructions",
            "Claude Code 用户指令",
            AgentAssetScope::User,
            true,
            "用户指令；当前会话是否已加载尚未验证",
        ))
        .is_break()
    {
        return;
    }
    for path in support::regular_files(root, &root.join("rules"), ".md") {
        let key = format!(
            "user-rule:{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        if output
            .emit(support::instruction(
                root,
                path,
                &key,
                "Claude Code 用户规则",
                AgentAssetScope::User,
                false,
                "按原生规则匹配与加载条件生效",
            ))
            .is_break()
        {
            return;
        }
    }
    if let Some(workspace) = request.workspace {
        for (index, directory) in support::ancestor_chain(workspace, None, output)
            .into_iter()
            .enumerate()
        {
            if directory.parent().is_none() {
                continue;
            }
            for relative in [
                "CLAUDE.md",
                ".claude/CLAUDE.md",
                "CLAUDE.local.md",
                "AGENTS.md",
            ] {
                let path = directory.join(relative);
                // A project below HOME can reach the user instruction file
                // again through its ancestor chain. Keep its one user source
                // and the original, narrower allowed root.
                if path == user_instructions {
                    continue;
                }
                let mut spec = support::instruction(
                    &directory,
                    path,
                    &format!("workspace-instructions:{index}:{relative}"),
                    "Claude Code 项目指令",
                    AgentAssetScope::Workspace,
                    directory == workspace && relative != "AGENTS.md",
                    "原生逐层指令与导入链；未声明当前会话已加载",
                );
                if relative == "AGENTS.md" {
                    spec.version_requirement = Some("AGENTS.md 需要 Claude Code v2.1.277+ 且满足原生兼容功能条件；不等同 CLAUDE.md".into());
                }
                if output.emit(spec).is_break() {
                    return;
                }
            }
        }
        for path in support::regular_files(workspace, &workspace.join(".claude/rules"), ".md") {
            let key = format!(
                "workspace-rule:{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            if output
                .emit(support::instruction(
                    workspace,
                    path,
                    &key,
                    "Claude Code 项目规则",
                    AgentAssetScope::Workspace,
                    false,
                    "按原生规则匹配范围加载",
                ))
                .is_break()
            {
                return;
            }
        }
    }
}
