mod config;
mod configuration;
mod environment;
mod launch;
mod sessions;

use super::discovery::paths::{
    node_cli_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, SessionAdapter, TemporaryLaunchAdapter,
        TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use crate::models::AgentCliKind;

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "PenguinHarness",
        executable: "penguin",
        session_name_hint: "",
        additional_env_keys: &[],
        home_scan,
        invalid_path_reason: None,
        require_version_substring: None,
        endpoint: EndpointAdapter::new(super::config_support::openai_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: false,
                session_resume: true,
                session_name: false,
            },
            launch::build_plan,
        )),
        sessions: Some(SessionAdapter::new(sessions::HISTORY, None)),
        liveness: None,
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::candidates,
        )),
        configuration: configuration::adapter(),
        environment: super::integration::environment("", environment::discover_contexts),
    }
}

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    node_cli_home_candidates(request, "penguin", std::iter::empty())
}
