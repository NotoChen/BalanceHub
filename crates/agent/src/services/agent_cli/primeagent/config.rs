use super::super::integration::expand_tilde;
use super::super::pi_family::{self, FamilySpec};
use crate::models::{AgentCliKind, CliConfigSnapshot, Provider};
use crate::services::agent_cli::configuration::contracts::ProviderConfigurationCandidate;
use crate::services::cli_paths::{configured_path, user_home};
use std::path::{Path, PathBuf};

pub(super) const SPEC: FamilySpec = FamilySpec {
    label: "Prime Agent",
    dir_for_home: config_dir_for_home,
    settings_schema: None,
    models_schema: None,
};

/// Prime Agent keeps Pi's files in `$PRIME_AGENT_CODING_AGENT_DIR` (a `~`
/// there stands for home; a relative path is its working directory's and is
/// not taken), else `~/.prime/agent`.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("PRIME_AGENT_CODING_AGENT_DIR")
        .map(|value| expand_tilde(&value.to_string_lossy(), home))
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home.join(".prime/agent"))
}

pub(super) fn config_dir() -> Option<PathBuf> {
    user_home().map(|home| config_dir_for_home(&home))
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    pi_family::settings_path(root)
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    pi_family::models_path(root)
}

/// The native session directory a provider launch points `--session-dir` at,
/// so temporary config isolation does not move session history. Scan and
/// launch share the same rules, so a relative override falls back instead of
/// being re-based on the project workdir.
pub(super) fn native_session_dir() -> Option<String> {
    let root = config_dir()?;
    Some(
        pi_family::sessions::session_directory(&root, &super::sessions::SPEC)
            .to_string_lossy()
            .into_owned(),
    )
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
