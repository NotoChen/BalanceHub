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
    vec![
        home.join(".grok/bin/grok"),
        home.join(".grok/bin/grok.exe"),
    ]
}
