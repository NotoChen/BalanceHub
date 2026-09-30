//! Claude Code's bounded, passive asset adapter. Raw snapshots are decoded by
//! the parser; resolver and assessor share the same typed native decisions.

mod control;
pub(super) mod discovery;
pub(super) mod parse;
pub(super) mod plugin;
mod policy;
mod resolve;

pub(crate) use control::ClaudeControlPayload;
pub(super) use discovery::{
    discover_contexts, discover_follow_up_sources, discover_resource_sources,
    discover_workspace_trust_sources, resolve_workspace_trust,
};
pub(super) use parse::parse_assets;
pub(crate) use plugin::ClaudePluginPayload;
pub(super) use resolve::{assess_assets, definition_suppressions, resolve_assets};

pub(super) const PARSER_VERSION: u32 = 4;
const SKILL_PREFIX: &str = "skill-manifest:";
const SETTINGS_AUTHORITIES: &[&str] = &[
    "settings",
    "workspace-settings",
    "workspace-local-settings",
    "managed-settings",
];

#[cfg(test)]
mod tests;

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
