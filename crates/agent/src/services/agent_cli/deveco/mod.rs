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
        label: "DevEco Code",
        executable: "deveco",
        session_name_hint: "",
        additional_env_keys: &[],
        home_scan,
        invalid_path_reason: None,
        require_version_substring: None,
        endpoint: EndpointAdapter::new(super::config_support::openai_base_url),
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
    let candidates = {
        let home = request.home();
        [
            home.join(".deveco/bin/deveco"),
            home.join(".deveco/bin/deveco.exe"),
        ]
    };
    node_cli_home_candidates(request, "deveco", candidates)
}
