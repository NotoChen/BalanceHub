pub(super) mod hooks;
pub(crate) use hooks::GrokHookPayload;
mod parse;
pub(super) mod plugin;
mod resolve;
mod skill;
mod trust;

pub(super) use parse::parse_assets;
pub(super) use plugin::definition_suppressions;
pub(crate) use plugin::GrokPluginPayload;
pub(super) use resolve::{assess_assets, resolve_assets};
pub(super) use trust::{discover_workspace_trust_sources, resolve_workspace_trust};

use super::config;
use crate::{
    models::{
        AgentAssetCategory, AgentAssetInstallationOrigin, AgentAssetScope, AgentAssetSourceKind,
    },
    services::agent_cli::{
        contracts::{
            AgentAssetSourcePathPolicy, AgentContextDiscoveryRequest, AgentDiagnosticOutput,
            AgentFollowUpSourceDiscoveryRequest, AgentSourceDiscoveryRequest, FollowUpSourceOutput,
            InitialSourceOutput,
        },
        environment::{default_context, source, SourceInput},
    },
};
use std::path::Path;

pub(super) const PARSER_VERSION: u32 = 3;

pub(super) fn discover_follow_up_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    if matches!(
        request.parent.native_source_key.as_str(),
        "hooks" | "workspace-hooks"
    ) {
        hooks::discover_sources(request, output);
    } else {
        skill::discover_follow_up_sources(request, output);
    }
}

pub(super) fn discover_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let root = config::config_dir_for_home(request.home);
    default_context(request, root, PARSER_VERSION)
}

pub(super) fn discover_resource_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    if let Some(workspace) = request.workspace {
        // The authority pass has already read these sources. Admit them before
        // eager package discovery so its shared source budget includes every read.
        discover_workspace_trust_sources(
            crate::services::agent_cli::contracts::AgentWorkspaceTrustSourceRequest {
                home: request.home,
                workspace,
                config_root: root,
            },
            output,
        );
    }
    let skill_directory_source = |input: SourceInput<'_>| {
        let mut spec = source(input);
        spec.path_policy = AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot {
            shared_root: request.home.join(".agents/skills"),
        };
        spec
    };
    macro_rules! emit {
        ($($value:expr),+ $(,)?) => {
            $(if output.emit_initial($value).is_break() { return; })+
        };
    }
    emit!(
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "config",
            label: "Grok Build 配置",
            path: root.join("config.toml"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Skill,
                AgentAssetCategory::Plugin,
                AgentAssetCategory::StatusUi,
                AgentAssetCategory::Hook,
            ],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "auth",
            label: "Grok Build 认证",
            path: root.join("auth.json"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        }),
        skill_directory_source(SourceInput {
            origin: AgentAssetInstallationOrigin::LocalFiles,
            native_source_key: "skills",
            label: "Grok Build Skills",
            path: root.join("skills"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Skill],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::SharedFiles,
            native_source_key: "shared-skills",
            label: "Grok Build 共享 Skills",
            path: request.home.join(".agents/skills"),
            allowed_root: request.home,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Skill],
        }),
        source(SourceInput {
            origin: AgentAssetInstallationOrigin::LocalFiles,
            native_source_key: "hooks",
            label: "Grok Build Hooks",
            path: root.join("hooks"),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::Directory,
            categories: &[AgentAssetCategory::Hook],
        }),
    );

    for (key, filename, label, definition) in [
        (
            hooks::POLICY_KEY,
            "disabled-hooks",
            "Grok Hook 禁用策略",
            false,
        ),
        (
            "hook-user-managed",
            "managed_config.toml",
            "Grok 用户托管 Hook 配置",
            true,
        ),
        (
            "hook-user-requirements",
            "requirements.toml",
            "Grok 用户 Hook 要求",
            true,
        ),
    ] {
        let mut spec = source(SourceInput {
            origin: AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: key,
            label,
            path: root.join(filename),
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: 10,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Hook],
        });
        spec.hook_definition_source = definition;
        if output.emit_initial(spec).is_break() {
            return;
        }
    }

    if let Some(workspace) = request.workspace {
        emit!(
            source(SourceInput {
                origin: AgentAssetInstallationOrigin::ConfigEntry,
                native_source_key: "workspace-config",
                label: "Grok Build 工作区配置",
                path: workspace.join(".grok/config.toml"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 20,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &[
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Plugin,
                    AgentAssetCategory::StatusUi,
                ],
            }),
            source(SourceInput {
                origin: AgentAssetInstallationOrigin::SharedFiles,
                native_source_key: "workspace-shared-skills",
                label: "Grok Build 工作区共享 Skills",
                path: workspace.join(".agents/skills"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 30,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[AgentAssetCategory::Skill],
            }),
            skill_directory_source(SourceInput {
                origin: AgentAssetInstallationOrigin::LocalFiles,
                native_source_key: "workspace-skills",
                label: "Grok Build 工作区 Skills",
                path: workspace.join(".grok/skills"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 40,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[AgentAssetCategory::Skill],
            }),
            source(SourceInput {
                origin: AgentAssetInstallationOrigin::LocalFiles,
                native_source_key: "workspace-hooks",
                label: "Grok Build 工作区 Hooks",
                path: workspace.join(".grok/hooks"),
                allowed_root: workspace,
                scope: AgentAssetScope::Workspace,
                precedence: 20,
                sensitive: true,
                source_kind: AgentAssetSourceKind::Directory,
                categories: &[AgentAssetCategory::Hook],
            }),
            hooks::workspace_file_source(workspace),
        );
    }
    plugin::discover_sources(request, output);
}

/// General settings and resource inventory consume the same native declarations.
pub(super) fn discover_sources(
    request: crate::services::agent_cli::contracts::AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::InitialSourceOutput,
) {
    crate::services::agent_cli::configuration::native_support::discover_inventory_sources(
        discover_resource_sources,
        super::configuration::discover_additional,
        request,
        output,
    );
}
