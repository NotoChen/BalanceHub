mod config;
mod configuration;
mod environment;
mod launch;
mod liveness;
mod sessions;

use super::discovery::paths::{
    node_cli_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
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
        label: "CodeBuddy",
        executable: "codebuddy",
        session_name_hint: "CodeBuddy 会话由原生会话 ID 继续；本次启动创建新会话",
        additional_env_keys: &[],
        home_scan,
        invalid_path_reason: None,
        require_version_substring: None,
        endpoint: EndpointAdapter::new(super::config_support::openai_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: true,
                session_resume: true,
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
        environment: super::integration::environment(
            "@tencent-ai/codebuddy-code",
            environment::discover_contexts,
        ),
    }
}

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    node_cli_home_candidates(request, "codebuddy", std::iter::empty())
}
