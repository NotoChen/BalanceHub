mod config;
mod configuration;
mod environment;
mod launch;
mod liveness;
mod sessions;

use super::discovery::paths::{
    fixed_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, LivenessAdapter, SessionAdapter,
        TemporaryLaunchAdapter, TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use crate::models::AgentCliKind;

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Mister Morph",
        executable: "morph",
        session_name_hint: "",
        additional_env_keys: &[],
        home_scan,
        invalid_path_reason: None,
        require_version_substring: None,
        endpoint: EndpointAdapter::new(super::config_support::openai_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: true,
                session_resume: false,
                session_name: false,
            },
            launch::build_plan,
        )),
        sessions: Some(SessionAdapter::new(sessions::HISTORY, None)),
        liveness: Some(LivenessAdapter::new(
            liveness::build_plan,
            liveness::parse_output,
        )),
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::candidates,
        )),
        configuration: configuration::adapter(),
        environment: super::integration::environment("", environment::discover_contexts),
    }
}

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    let home = request.home();
    let candidates = [
        home.join(".local/bin/morph"),
        home.join(".local/bin/mistermorph"),
        home.join("go/bin/morph"),
        home.join("go/bin/mistermorph"),
    ];
    fixed_home_candidates(request, candidates)
}
