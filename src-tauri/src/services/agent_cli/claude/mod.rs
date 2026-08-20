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
    }
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = node_cli_home_candidates(home, "claude");
    candidates.push(home.join(".claude/local/claude"));
    candidates
}
