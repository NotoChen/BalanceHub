//! Codex 0.154 native Skill and Plugin source/schema ownership.

mod mcp;
mod plugin;
mod skill;
mod sources;

#[cfg(test)]
mod tests;

pub(in crate::services::agent_cli::codex) use plugin::hooks as plugin_hooks;
pub(super) use plugin::{
    active_version, decode_plugin_config, plugin_id_parts, CONFIG_ROOT_ID as PLUGIN_CONFIG_ROOT_ID,
    CONFIG_ROOT_KEY as PLUGIN_CONFIG_ROOT_KEY,
};
pub(in crate::services::agent_cli::codex) use skill::definition_suppressions;
pub(super) use skill::{RULES_ID as SKILL_RULES_ID, RULES_KEY as SKILL_RULES_KEY};
pub(in crate::services::agent_cli::codex) use sources::discover_follow_up_sources;
pub(super) use sources::{
    discover_sources, plugin_manifest_parts, plugin_mcp_source_parts, plugin_skill_source_parts,
    skill_source_parts,
};

use crate::models::{AgentAssetDiagnostic, AgentAssetDocumentFormat};
use crate::services::agent_cli::contracts::{
    AgentAssetParseRequest, AgentAssetSnapshot, AgentDiagnosticOutput, AgentOutputStop,
    AgentParseOutput,
};
use std::ops::ControlFlow;

pub(super) const SKILL_PREFIX: &str = "codex-skill:";
pub(super) const PLUGIN_BASE_PREFIX: &str = "codex-plugin-base:";
pub(super) const PLUGIN_MANIFEST_PREFIX: &str = "codex-plugin-manifest:";
pub(super) const PLUGIN_SKILLS_PREFIX: &str = "codex-plugin-skills:";
pub(super) const PLUGIN_MCP_PREFIX: &str = "codex-plugin-mcp:";

pub(super) fn parse_source(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    let key = request.source.native_source_key.as_str();
    if key.starts_with(SKILL_PREFIX) {
        skill::parse_manifest(request, output);
    } else if key.starts_with(PLUGIN_BASE_PREFIX) {
        plugin::parse_versions(request, output);
    } else if key.starts_with(PLUGIN_MANIFEST_PREFIX) {
        plugin::parse_manifest(request, output);
    } else if key.starts_with(PLUGIN_MCP_PREFIX) {
        plugin::parse_mcp_file(request, output);
    } else if key.starts_with(plugin_hooks::SOURCE_PREFIX) {
        plugin_hooks::parse_file(request, output);
    } else {
        return false;
    }
    true
}

pub(super) fn parse_config(
    request: AgentAssetParseRequest<'_>,
    root: Option<&toml::value::Table>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if request.source.native_source_key == "config" {
        skill::parse_rules(request, root, output)?;
    }
    plugin::parse_config(request, root, output)
}

fn snapshot_toml(snapshot: &AgentAssetSnapshot) -> Option<toml::value::Table> {
    let AgentAssetSnapshot::File { bytes, .. } = snapshot else {
        return None;
    };
    std::str::from_utf8(bytes)
        .ok()?
        .parse::<toml::Value>()
        .ok()?
        .as_table()
        .cloned()
}

fn malformed(output: &mut dyn AgentDiagnosticOutput, format: AgentAssetDocumentFormat) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format,
        location: None,
    });
}
