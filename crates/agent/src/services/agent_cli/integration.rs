//! Shared scaffolding for integration-tier Agent CLIs.
//!
//! These Agents expose native configuration and, where the CLI's launch flags
//! are verified, a temporary launch plan. Their MCP declarations are read from
//! ordinary native documents by the shared integration inventory; Skill and
//! Hook editing stay off because those surfaces need Agent-specific enablement
//! and mutation semantics.

use super::contracts::{AgentContextDiscovery, AgentContextDiscoveryRequest, EnvironmentAdapter};
use super::environment::generic_mcp;
use crate::models::AgentConfigurationContext;
use std::path::{Path, PathBuf};

pub(crate) const fn environment(
    package_name: &'static str,
    discover_contexts: AgentContextDiscovery,
) -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        discover_contexts,
        generic_mcp::discover_sources,
        None,
        package_name,
        generic_mcp::parse_assets,
        generic_mcp::resolve_assets,
        generic_mcp::assess_assets,
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

/// API keys and model IDs reach native config and process arguments. Reject
/// control characters before either path so a pasted secret cannot break the
/// surrounding document or CLI invocation.
pub(crate) fn validate_native_value(value: &str, label: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        return Err(format!("{label}不能包含控制字符"));
    }
    Ok(())
}
