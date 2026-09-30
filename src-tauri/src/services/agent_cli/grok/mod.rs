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

pub(crate) use environment::{GrokHookPayload, GrokPluginPayload};

use super::discovery::paths::{
    fixed_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::native_agent_kinds::grok::AGENT_KIND;
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
        label: "Grok Build",
        executable: "grok",
        session_name_hint: "Grok Build 不支持启动前命名；启动后可在终端输入 /rename",
        additional_env_keys: &["GROK_CLI_PATH"],
        home_scan,
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
            "@xai-official/grok",
            environment::parse_assets,
            environment::resolve_assets,
            environment::assess_assets,
        )
        .with_workspace_trust_authority(
            environment::discover_workspace_trust_sources,
            environment::resolve_workspace_trust,
        )
        .with_definition_selector(environment::definition_suppressions)
        .with_catalog_adapter(&super::catalog::native::GROK)
        .with_hook_adapter(&hook_catalog::ADAPTER)
        .with_source_preview(asset_preview::source_preview)
        .with_asset_mutation(
            asset_mutation::mechanisms,
            asset_mutation::prepare,
            asset_mutation::unavailable_reason,
        )
        .with_macos_vendor_signature("5Y6N3AJ54S", "xai-grok-pager"),
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

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    let candidates = [
        request.home().join(".grok/bin/grok"),
        request.home().join(".grok/bin/grok.exe"),
    ];
    fixed_home_candidates(request, candidates)
}
