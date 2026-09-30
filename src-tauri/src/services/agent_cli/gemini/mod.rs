mod asset_mutation;
mod asset_preview;
mod config;
mod configuration;
mod environment;
mod hook_catalog;
mod launch;
mod liveness;
mod native_context;
mod sessions;

use super::discovery::paths::{
    node_cli_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, EnvironmentAdapter, LivenessAdapter, SessionAdapter,
        TemporaryLaunchAdapter, TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use crate::models::AgentCliKind;

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Gemini CLI",
        executable: "gemini",
        session_name_hint: "Gemini CLI 没有启动前会话命名参数，标题由 Gemini 自动生成",
        additional_env_keys: &["GEMINI_CLI_PATH"],
        home_scan,
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
        sessions: Some(SessionAdapter::new(
            sessions::HISTORY,
            Some(sessions::metadata_lookup),
        )),
        liveness: Some(LivenessAdapter::new(
            liveness::build_plan,
            liveness::parse_output,
        )),
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::candidates,
        )),
        configuration: configuration::adapter(),
        environment: EnvironmentAdapter::with_pipeline(
            environment::discover_contexts,
            environment::discover_sources,
            Some(environment::discover_follow_up_sources),
            "@google/gemini-cli",
            environment::parse_assets,
            environment::resolve_assets,
            environment::assess_assets,
        )
        .with_readonly_external_roots(environment::readonly_external_roots)
        .with_catalog_adapter(&super::catalog::native::GEMINI)
        .with_hook_adapter(&hook_catalog::ADAPTER)
        .with_source_preview(asset_preview::source_preview)
        .with_asset_mutation(
            asset_mutation::mechanisms,
            asset_mutation::prepare,
            asset_mutation::unavailable_reason,
        )
        .with_workspace_trust_authority(
            environment::discover_workspace_trust_sources,
            environment::resolve_workspace_trust,
        ),
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

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    node_cli_home_candidates(request, "gemini", std::iter::empty())
}
