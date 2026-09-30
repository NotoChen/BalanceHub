mod asset_mutation;
mod asset_preview;
mod config;
mod configuration;
mod environment;
mod hook_catalog;
mod native_context;
pub(crate) use environment::CodexHookPayload;
mod launch;
mod liveness;
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
use std::path::Path;

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Codex CLI",
        executable: "codex",
        session_name_hint:
            "Codex CLI 当前不支持启动前命名；启动后可在终端输入 /new 名称 或 /rename",
        additional_env_keys: &["CODEX_CLI_PATH"],
        home_scan,
        invalid_path_reason: Some(invalid_path_reason),
        require_version_substring: None,
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
            "@openai/codex",
            environment::parse_assets,
            environment::resolve_assets,
            environment::assess_assets,
        )
        .with_definition_selector(environment::definition_suppressions)
        .with_catalog_adapter(&super::catalog::native::CODEX)
        .with_hook_adapter(&hook_catalog::ADAPTER)
        .with_source_preview(asset_preview::source_preview)
        .with_readonly_external_roots(environment::readonly_external_roots)
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
    let normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return normalized;
    }
    if normalized.ends_with("/v1") {
        normalized
    } else {
        format!("{normalized}/v1")
    }
}

fn home_scan(request: AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult {
    let native = request.home().join(".codex/bin/codex");
    node_cli_home_candidates(request, "codex", [native])
}

fn invalid_path_reason(path: &Path) -> Option<&'static str> {
    let value = path.to_string_lossy().replace('\\', "/");
    value
        .contains(".app/Contents/")
        .then_some("不支持使用 Codex Desktop App 内置二进制，请安装并选择独立的 codex CLI")
}
