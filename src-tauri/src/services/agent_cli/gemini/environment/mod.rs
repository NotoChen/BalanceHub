//! Gemini CLI's bounded, passive asset inventory.
//!
//! The files in this module are deliberately split by ownership.  This keeps
//! platform paths, trust authority, decoding, parsing and projection logic
//! independently reviewable while preserving one adapter entrypoint.

mod builtin;
mod decode;
pub(super) mod hook_policy;
pub(super) mod hooks;
mod parse;
mod resolve;
mod skill;
mod sources;
mod trust;

pub(super) use builtin::readonly_external_roots;

#[cfg(test)]
mod tests;

pub(super) fn parse_assets(
    request: crate::services::agent_cli::contracts::AgentAssetParseRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::AgentParseOutput,
) {
    if !request.source.categories.is_empty() {
        decode::parse_assets(request, output);
    }
}

pub(super) fn resolve_assets(
    request: crate::services::agent_cli::contracts::AgentAssetResolveRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::AgentResolveOutput,
) {
    resolve::resolve_assets(request, output);
}

pub(super) fn assess_assets(
    request: crate::services::agent_cli::contracts::AgentAssetAssessmentRequest<'_>,
) -> crate::services::agent_cli::contracts::AgentAssetAssessmentIndex {
    resolve::assess_assets(request)
}

pub(super) fn discover_contexts(
    request: crate::services::agent_cli::contracts::AgentContextDiscoveryRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    sources::discover_contexts(request, output)
}

pub(super) fn discover_resource_sources(
    request: crate::services::agent_cli::contracts::AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::InitialSourceOutput,
) {
    builtin::discover_sources(request, output);
    sources::discover_sources(request, output);
}

pub(super) fn discover_follow_up_sources(
    request: crate::services::agent_cli::contracts::AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::FollowUpSourceOutput,
) {
    sources::discover_follow_up_sources(request, output);
}

pub(super) fn discover_workspace_trust_sources(
    request: crate::services::agent_cli::contracts::AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::InitialSourceOutput,
) {
    trust::discover_workspace_trust_sources(request, output);
}

pub(super) fn resolve_workspace_trust(
    request: crate::services::agent_cli::contracts::AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::AgentDiagnosticOutput,
) -> crate::models::AgentTrustState {
    trust::resolve_workspace_trust(request, output)
}

pub(super) fn mcp_participation(
    request: crate::services::agent_cli::contracts::AgentAssetParseRequest<'_>,
) -> crate::models::AgentAssetResolutionParticipation {
    sources::mcp_participation(request)
}

pub(super) fn participation(
    request: crate::services::agent_cli::contracts::AgentAssetParseRequest<'_>,
    category: crate::models::AgentAssetCategory,
) -> crate::models::AgentAssetResolutionParticipation {
    sources::participation(request, category)
}

pub(super) fn emit_malformed(
    output: &mut dyn crate::services::agent_cli::contracts::AgentDiagnosticOutput,
    location: &str,
) {
    decode::emit_malformed(output, location);
}

pub(super) fn parse_gemini_jsonc(
    bytes: &[u8],
    output: &mut dyn crate::services::agent_cli::contracts::AgentDiagnosticOutput,
    category: crate::models::AgentAssetCategory,
) -> Option<serde_json::Value> {
    decode::parse_gemini_jsonc(bytes, output, category)
}

pub(super) fn parse_strict_json(
    bytes: &[u8],
    output: &mut dyn crate::services::agent_cli::contracts::AgentDiagnosticOutput,
    category: crate::models::AgentAssetCategory,
) -> Option<serde_json::Value> {
    decode::parse_strict_json(bytes, output, category)
}

pub(super) const PARSER_VERSION: u32 = 3;
pub(super) const EXTENSIONS_SOURCE_KEY: &str = "extensions";
pub(super) const EXTENSION_MANIFEST_PREFIX: &str = "gemini-extension-manifest:";
pub(super) const EXTENSION_HOOKS_PREFIX: &str = "gemini-extension-hooks:";
pub(super) const EXTENSION_SKILLS_PREFIX: &str = "gemini-extension-skills:";
pub(super) const EXTENSION_SKILL_PREFIX: &str = "gemini-extension-skill:";
pub(super) const SKILL_MANIFEST_PREFIX: &str = "gemini-skill-manifest:";

/// General settings and resource inventory consume the same native declarations.
pub(super) fn discover_sources(
    request: crate::services::agent_cli::contracts::AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::InitialSourceOutput,
) {
    crate::services::agent_cli::configuration::native_support::discover_inventory_sources(
        discover_resource_sources,
        super::configuration::discover_additional,
        request,
        output,
    );
}
