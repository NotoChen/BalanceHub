use super::environment;
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
            "config",
            "workspace-config",
            "auth",
            "hook-user-managed",
            "hook-user-requirements",
        ],
    ) {
        let may_create = !matches!(
            native.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) && !matches!(
            native.native_source_key.as_str(),
            "hook-user-managed" | "hook-user-requirements"
        );
        let auth = native.native_source_key == "auth";
        let load_rule = match native.native_source_key.as_str() {
            "hook-user-managed" => "managed_config.toml 提供低优先级默认值",
            "hook-user-requirements" => "requirements.toml 提供不能被普通用户配置覆盖的约束",
            "workspace-config" => {
                "项目层只贡献 mcp_servers、plugins、permission；不是用户配置的完整覆盖"
            }
            _ => "原生用户来源；环境变量、策略与启动参数可能改变实际采用值",
        };
        let mut spec = support::describe(
            native,
            if auth {
                AgentConfigurationFormat::Json
            } else {
                AgentConfigurationFormat::Toml
            },
            may_create,
            load_rule,
            "保存后，请在新 Grok 会话中确认加载结果",
        );
        if auth {
            spec.initial_text = Some("{}\n".to_owned());
        }
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
        .emit(support::instruction(
            root,
            root.join("AGENTS.md"),
            "user-instructions",
            "Grok Build 用户指令",
            AgentAssetScope::User,
            true,
            "全局指令候选；原生兼容扫描配置可能改变是否加载",
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
                "Grok Build 用户规则",
                AgentAssetScope::User,
                false,
                "原生全局规则来源",
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
            if output
                .emit(support::instruction(
                    &directory,
                    directory.join("AGENTS.md"),
                    &format!("workspace-instructions:{index}"),
                    "Grok Build 项目指令",
                    AgentAssetScope::Workspace,
                    directory == workspace,
                    "仓库根至当前目录的指令候选；深层优先且受原生 ignore/兼容规则影响",
                ))
                .is_break()
            {
                return;
            }
        }
        for path in support::regular_files(workspace, &workspace.join(".grok/rules"), ".md") {
            let key = format!(
                "workspace-rule:{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            if output
                .emit(support::instruction(
                    workspace,
                    path,
                    &key,
                    "Grok Build 项目规则",
                    AgentAssetScope::Workspace,
                    false,
                    "原生项目规则；忽略和加载状态尚未验证",
                ))
                .is_break()
            {
                return;
            }
        }
    }
}
