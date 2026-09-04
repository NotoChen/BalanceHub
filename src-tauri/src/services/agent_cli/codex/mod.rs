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
use crate::models::{AgentCliKind, CliSessionSummary};
use std::path::{Path, PathBuf};

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Codex CLI",
        executable: "codex",
        session_name_hint:
            "Codex CLI 当前不支持启动前命名；启动后可在终端输入 /new 名称 或 /rename",
        additional_env_keys: &["CODEX_CLI_PATH"],
        home_candidates,
        invalid_path_reason: Some(invalid_path_reason),
        require_version_substring: None,
        endpoint: EndpointAdapter::new(normalize_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: true,
                session_resume: true,
                session_name: false,
            },
            None,
            launch::build_plan,
        )),
        sessions: Some(SessionAdapter::new(
            list_sessions,
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
        environment: EnvironmentAdapter::new(discover_assets, "@openai/codex"),
    }
}

fn discover_assets(
    home: &Path,
    workspace: Option<&Path>,
) -> Vec<super::contracts::AgentAssetDeclaration> {
    const TEMPLATES: &[super::environment::AssetTemplate] = &[
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "config",
            label: "Codex 配置",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/config.toml",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "auth",
            label: "Codex 认证",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/auth.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "skills",
            label: "Codex Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".agents/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 30,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "codex-skills",
            label: "Codex Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Plugin,
            native_id: "plugins",
            label: "Codex Plugins",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/plugins",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "hooks",
            label: "Codex Hooks",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/hooks.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "status-line",
            label: "Codex Status UI",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/config.toml",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "mcp",
            label: "Codex MCP",
            root: super::environment::AssetRoot::User,
            relative_path: ".codex/config.toml",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "workspace-config",
            label: "Codex 工作区配置",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".codex/config.toml",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "workspace-skills",
            label: "Codex 工作区 Skills",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".agents/skills",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "workspace-hooks",
            label: "Codex 工作区 Hooks",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".codex/hooks.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "workspace-status-line",
            label: "Codex 工作区 Status UI",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".codex/config.toml",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "workspace-mcp",
            label: "Codex 工作区 MCP",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".codex/config.toml",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
    ];
    super::environment::from_templates(home, workspace, TEMPLATES)
}

fn normalize_base_url(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return normalized;
    }
    if normalized.ends_with("/v1") {
        normalized
    } else {
        format!("{normalized}/v1")
    }
}

fn list_sessions(cli_kind: AgentCliKind, workdir: &Path) -> Result<Vec<CliSessionSummary>, String> {
    sessions::list(cli_kind, workdir, 100)
}

fn home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = node_cli_home_candidates(home, "codex");
    candidates.push(home.join(".codex/bin/codex"));
    candidates
}

fn invalid_path_reason(path: &Path) -> Option<&'static str> {
    let value = path.to_string_lossy().replace('\\', "/");
    value
        .contains(".app/Contents/")
        .then_some("不支持使用 Codex Desktop App 内置二进制，请安装并选择独立的 codex CLI")
}
