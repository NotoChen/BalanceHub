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

pub(crate) use environment::ClaudeControlPayload;
pub(crate) use environment::ClaudePluginPayload;

use super::discovery::paths::{
    node_cli_home_candidates, AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult,
};
use super::native_agent_kinds::claude::AGENT_KIND;
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
        label: "Claude Code",
        executable: "claude",
        session_name_hint: "",
        additional_env_keys: &["CLAUDE_CODE_CLI_PATH", "CLAUDE_CLI_PATH"],
        home_scan,
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
            "@anthropic-ai/claude-code",
            environment::parse_assets,
            environment::resolve_assets,
            environment::assess_assets,
        )
        .with_catalog_adapter(&super::catalog::native::CLAUDE)
        .with_hook_adapter(&hook_catalog::ADAPTER)
        .with_definition_selector(environment::definition_suppressions)
        .with_source_preview(asset_preview::source_preview)
        .with_asset_mutation(
            asset_mutation::mechanisms,
            asset_mutation::prepare,
            asset_mutation::unavailable_reason,
        )
        .with_macos_vendor_signature("Q6L2SF6YDW", "com.anthropic.claude-code")
        .with_workspace_trust_authority(
            environment::discover_workspace_trust_sources,
            environment::resolve_workspace_trust,
        ),
    }
}
fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    let native = request.home().join(".claude/local/claude");
    node_cli_home_candidates(request, "claude", [native])
}
