mod config;
mod configuration;
mod environment;
mod launch;

use super::discovery::paths::{
    node_cli_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, TemporaryLaunchAdapter, TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use crate::models::AgentCliKind;

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Qwen Code",
        executable: "qwen",
        session_name_hint: "Qwen Code 暂未接入会话工作台；本次启动只能创建新会话",
        additional_env_keys: &[],
        home_scan,
        invalid_path_reason: None,
        // 官方 `qwen --version` 是否附带产品名随版本变化，按版本号识别。
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
        sessions: None,
        liveness: None,
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::candidates,
        )),
        configuration: configuration::adapter(),
        environment: super::integration::environment(
            "@qwen-code/qwen-code",
            environment::discover_contexts,
        ),
    }
}

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    node_cli_home_candidates(request, "qwen", std::iter::empty())
}
