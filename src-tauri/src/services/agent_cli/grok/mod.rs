mod config;
mod launch;
mod liveness;
mod sessions;

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
        label: "Grok Build",
        executable: "grok",
        session_name_hint: "Grok Build 不支持启动前命名；启动后可在终端输入 /rename",
        additional_env_keys: &["GROK_CLI_PATH"],
        home_candidates,
        invalid_path_reason: None,
        require_version_substring: Some("grok"),
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
        environment: EnvironmentAdapter::new(discover_assets, "@xai-official/grok"),
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
            label: "Grok Build 配置",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/config.toml",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "auth",
            label: "Grok Build 认证",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/auth.json",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "skills",
            label: "Grok Build Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "agent-skills",
            label: "Grok Build 共享 Skills",
            root: super::environment::AssetRoot::User,
            relative_path: ".agents/skills",
            scope: crate::models::AgentAssetScope::User,
            precedence: 30,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Plugin,
            native_id: "plugins",
            label: "Grok Build Plugins",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/plugins",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "mcp",
            label: "Grok Build MCP",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/config.toml",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "hooks",
            label: "Grok Build Hooks",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/hooks",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "status-line",
            label: "Grok Build Status UI",
            root: super::environment::AssetRoot::User,
            relative_path: ".grok/config.toml",
            scope: crate::models::AgentAssetScope::User,
            precedence: 20,
            sensitive: false,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "workspace-skills",
            label: "Grok Build 工作区 Skills",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".agents/skills",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 5,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Skill,
            native_id: "workspace-grok-skills",
            label: "Grok Build 工作区 Skills",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".grok/skills",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Config,
            native_id: "workspace-config",
            label: "Grok Build 工作区配置",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".grok/config.toml",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Plugin,
            native_id: "workspace-plugins",
            label: "Grok Build 工作区 Plugins",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".grok/plugins",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Hook,
            native_id: "workspace-hooks",
            label: "Grok Build 工作区 Hooks",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".grok/hooks",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: false,
            is_directory: true,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::Mcp,
            native_id: "workspace-mcp",
            label: "Grok Build 工作区 MCP",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".grok/config.toml",
            scope: crate::models::AgentAssetScope::Workspace,
            precedence: 10,
            sensitive: true,
            is_directory: false,
        },
        super::environment::AssetTemplate {
            category: crate::models::AgentAssetCategory::StatusUi,
            native_id: "workspace-status-line",
            label: "Grok Build 工作区 Status UI",
            root: super::environment::AssetRoot::Workspace,
            relative_path: ".grok/config.toml",
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
    if normalized.ends_with("/v1") {
        normalized.to_string()
    } else {
        format!("{normalized}/v1")
    }
}

fn home_candidates(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".grok/bin/grok"), home.join(".grok/bin/grok.exe")]
}
