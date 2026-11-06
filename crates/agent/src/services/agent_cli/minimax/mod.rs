mod config;
mod configuration;
mod environment;

use super::discovery::paths::{
    node_cli_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::{
    contracts::{DefaultConfigAdapter, EndpointAdapter},
    AgentCliDefinition,
};
use crate::models::AgentCliKind;

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "MiniMax Code",
        executable: "mcode",
        session_name_hint: "",
        additional_env_keys: &[],
        home_scan,
        invalid_path_reason: None,
        require_version_substring: None,
        // Anthropic Messages SDK appends /v1/messages to the base URL.
        endpoint: EndpointAdapter::new(|base_url| {
            base_url.trim().trim_end_matches('/').to_string()
        }),
        temporary_launch: None,
        sessions: None,
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
    node_cli_home_candidates(request, "mcode", std::iter::empty())
}
