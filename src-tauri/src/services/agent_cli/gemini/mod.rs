mod config;
mod launch;
mod liveness;
mod sessions;

use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, LivenessAdapter, SessionAdapter,
        TemporaryLaunchAdapter, TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use super::discovery::paths::node_cli_home_candidates;
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
        sessions: Some(SessionAdapter::new(sessions::list)),
        liveness: Some(LivenessAdapter::new(
            liveness::build_plan,
            liveness::parse_output,
        )),
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::preview,
            config::switch,
        )),
    }
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
