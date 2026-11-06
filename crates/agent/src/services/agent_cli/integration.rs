//! Shared scaffolding for integration-tier Agent CLIs.
//!
//! These Agents expose native configuration and, where the CLI's launch flags
//! are verified, a temporary launch plan, but do not publish an asset inventory
//! yet. An empty environment pipeline keeps the MCP / Skill / Hook surfaces off
//! without inventing parser semantics; each Agent still owns its native config
//! paths and formats.

use super::contracts::{
    AgentAssetAssessmentIndex, AgentAssetAssessmentRequest, AgentAssetParseRequest,
    AgentAssetResolveRequest, AgentContextDiscovery, AgentContextDiscoveryRequest,
    AgentParseOutput, AgentResolveOutput, AgentSourceDiscoveryRequest, EnvironmentAdapter,
    InitialSourceOutput,
};
use crate::models::AgentConfigurationContext;
use std::path::{Path, PathBuf};

pub(crate) const fn environment(
    package_name: &'static str,
    discover_contexts: AgentContextDiscovery,
) -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        discover_contexts,
        no_sources,
        None,
        package_name,
        no_parse,
        no_resolve,
        no_assess,
    )
}

/// `$XDG_CONFIG_HOME`, or the platform default beneath `home`. A relative
/// value is not a valid XDG base directory and is ignored.
pub(crate) fn xdg_config_home(home: &Path) -> PathBuf {
    xdg_home("XDG_CONFIG_HOME", home, ".config")
}

/// `$XDG_DATA_HOME`, or the platform default beneath `home`.
pub(crate) fn xdg_data_home(home: &Path) -> PathBuf {
    xdg_home("XDG_DATA_HOME", home, ".local/share")
}

fn xdg_home(key: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
}

/// Expand a leading `~` in an environment-supplied path. Native CLIs accept
/// both separators in that position, so both are handled.
pub(crate) fn expand_tilde(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home.join(rest);
    }
    PathBuf::from(value)
}

pub(crate) fn contexts(
    request: AgentContextDiscoveryRequest<'_>,
    config_root: PathBuf,
    parser_version: u32,
) -> Vec<AgentConfigurationContext> {
    super::environment::default_context(request, config_root, parser_version)
}

fn no_sources(_: AgentSourceDiscoveryRequest<'_>, _: &mut dyn InitialSourceOutput) {}

fn no_parse(_: AgentAssetParseRequest<'_>, _: &mut dyn AgentParseOutput) {}

fn no_resolve(_: AgentAssetResolveRequest<'_>, _: &mut dyn AgentResolveOutput) {}

fn no_assess(_: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    AgentAssetAssessmentIndex::default()
}

/// API keys and model IDs reach native config and process arguments. Reject
/// control characters before either path so a pasted secret cannot break the
/// surrounding document or CLI invocation.
pub(crate) fn validate_native_value(value: &str, label: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label}不能包含控制字符"));
    }
    Ok(())
}
