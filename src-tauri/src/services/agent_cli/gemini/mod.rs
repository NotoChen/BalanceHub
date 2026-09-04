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
        label: "Gemini CLI",
        executable: "gemini",
        session_name_hint: "Gemini CLI 没有启动前会话命名参数，标题由 Gemini 自动生成",
        additional_env_keys: &["GEMINI_CLI_PATH"],
        home_candidates,
        invalid_path_reason: None,
        // 官方 `gemini --version` 只输出版本号，例如 `0.55.1`。
        require_version_substring: None,
        endpoint: EndpointAdapter::new(normalize_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: true,
                session_resume: true,
                session_name: false,
            },
            Some("gemini-system-settings.json"),
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
        environment: EnvironmentAdapter::new(discover_assets, "@google/gemini-cli"),
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
            label: "Gemini CLI 设置",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "env",
            label: "Gemini CLI 环境配置",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/.env",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "skills",
            label: "Gemini CLI Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "agent-skills",
            label: "Gemini CLI 共享 Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".agents/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 30,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Extension,
            native_id: "extensions",
            label: "Gemini CLI Extensions",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/extensions",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "mcp",
            label: "Gemini CLI MCP",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "hooks",
            label: "Gemini CLI Hooks",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "footer",
            label: "Gemini CLI Status UI",
            root: super::environment::AssetRoot::User,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "workspace-skills",
            label: "Gemini CLI 工作区 Skills",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".agents/skills",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 5,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "workspace-gemini-skills",
            label: "Gemini CLI 工作区 Skills",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".gemini/skills",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "workspace-settings",
            label: "Gemini CLI 工作区设置",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "workspace-mcp",
            label: "Gemini CLI 工作区 MCP",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "workspace-hooks",
            label: "Gemini CLI 工作区 Hooks",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "workspace-footer",
            label: "Gemini CLI 工作区 Status UI",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".gemini/settings.json",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
    ];
    super::environment::from_templates(home, workspace, TEMPLATES)
}

fn normalize_base_url(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return String::new();
    }
    normalized
        .strip_suffix("/v1beta")
        .or_else(|| normalized.strip_suffix("/v1"))
        .unwrap_or(normalized)
        .trim_end_matches('/')
        .to_string()
}

fn home_candidates(home: &Path) -> Vec<PathBuf> {
    node_cli_home_candidates(home, "gemini")
}
