mod config;
mod launch;
mod liveness;
mod sessions;

use super::discovery::paths::node_cli_home_candidates;
use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, EnvironmentAdapter, LivenessAdapter, SessionAdapter,
        TemporaryLaunchAdapter, TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use crate::models::AgentCliKind;
use std::path::{Path, PathBuf};

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Claude Code",
        executable: "claude",
        session_name_hint: "",
        additional_env_keys: &["CLAUDE_CODE_CLI_PATH", "CLAUDE_CLI_PATH"],
        home_candidates,
        invalid_path_reason: None,
        require_version_substring: Some("claude"),
        endpoint: EndpointAdapter::new(normalize_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: true,
                session_resume: true,
                session_name: true,
            },
            Some("claude-settings.json"),
            launch::build_plan,
        )),
        sessions: Some(SessionAdapter::new(
            sessions::list,
            Some(sessions::search),
            Some(sessions::detail),
            Some(sessions::index),
            Some(sessions::metadata_lookup),
        )),
        liveness: Some(LivenessAdapter::new(
            liveness::build_plan,
            liveness::parse_output,
        )),
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::preview,
            config::switch,
        )),
        environment: EnvironmentAdapter::new(discover_assets, "@anthropic-ai/claude-code"),
    }
}

fn discover_assets(
    home: &Path,
    workspace: Option<&Path>,
) -> Vec<super::contracts::AgentAssetDeclaration> {
    const TEMPLATES: &[super::environment::AssetTemplate] = &[
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "settings",
            label: "Claude Code 设置",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "account",
            label: "Claude Code 账户设置",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 30,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "skills",
            label: "Claude Code Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Plugin,
            native_id: "plugins",
            label: "Claude Code Plugins",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude/plugins",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "user-mcp-state",
            label: "Claude Code 用户 MCP 状态",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "hooks",
            label: "Claude Code Hooks",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "status-line",
            label: "Claude Code Status UI",
            root: super::environment::AssetRoot::User,
            relative_path: ".claude/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "workspace-settings",
            label: "Claude Code 工作区设置",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".claude/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "workspace-local-settings",
            label: "Claude Code 工作区本地设置",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".claude/settings.local.json",
            scope: crate::models::AgentAssetScope::Local,
            precedence: 5,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "workspace-skills",
            label: "Claude Code 工作区 Skills",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".claude/skills",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "workspace-mcp",
            label: "Claude Code 工作区 MCP",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".mcp.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 5,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "workspace-hooks",
            label: "Claude Code 工作区 Hooks",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".claude/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "workspace-local-hooks",
            label: "Claude Code 工作区本地 Hooks",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".claude/settings.local.json",
            scope: crate::models::AgentAssetScope::Local,
            precedence: 5,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "workspace-status-line",
            label: "Claude Code 工作区 Status UI",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".claude/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
    ];
    super::environment::from_templates(home, workspace, TEMPLATES)
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = node_cli_home_candidates(home, "claude");
    candidates.push(home.join(".claude/local/claude"));
    candidates
}
