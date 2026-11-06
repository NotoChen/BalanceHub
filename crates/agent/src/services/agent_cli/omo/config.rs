use super::super::pi_family::{self, FamilySpec};
use crate::models::{AgentCliKind, CliConfigSnapshot, Provider};
use crate::services::agent_cli::configuration::contracts::ProviderConfigurationCandidate;
use crate::services::cli_paths::configured_path;
use std::path::{Path, PathBuf};

pub(super) const SPEC: FamilySpec = FamilySpec {
    label: "OmO",
    dir_for_home: agent_dir_for_home,
    settings_schema: None,
    models_schema: None,
};

/// OmO keeps Pi's files in `$OMO_CODING_AGENT_DIR`, else
/// `$SENPI_CODING_AGENT_DIR` (absolute only), else `~/.omo/agent`. It also
/// reads Pi's `$PI_CODING_AGENT_DIR`, but a folder both share belongs to the
/// Pi row, so it is not followed here.
pub(super) fn agent_dir_for_home(home: &Path) -> PathBuf {
    configured_path("OMO_CODING_AGENT_DIR")
        .or_else(|| configured_path("SENPI_CODING_AGENT_DIR"))
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home.join(".omo/agent"))
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    pi_family::settings_path(root)
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    pi_family::models_path(root)
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    pi_family::snapshot(&SPEC, cli_kind, providers)
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    pi_family::candidates(&SPEC, cli_kind, provider, api_key_local_id)
}
