use super::super::pi_family::{self, FamilySpec};
use crate::models::{AgentCliKind, CliConfigSnapshot, Provider};
use crate::services::agent_cli::configuration::contracts::ProviderConfigurationCandidate;
use crate::services::cli_paths::{configured_path, user_home};
use std::path::{Path, PathBuf};

pub(super) const SPEC: FamilySpec = FamilySpec {
    label: "OmO",
    dir_for_home: agent_dir_for_home,
    settings_schema: None,
    models_schema: None,
};

pub(super) const PROVIDER_ID: &str = "balancehub";

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

pub(super) fn config_dir() -> Option<PathBuf> {
    user_home().map(|home| agent_dir_for_home(&home))
}

/// OmO's native session directory. Isolating `OMO_CODING_AGENT_DIR` for a
/// temporary launch would otherwise move new sessions into the launch folder;
/// the explicit `--session-dir` keeps them in the user's real location. Scan
/// and launch share the same rules, so a relative override is not re-based on
/// the project workdir.
pub(super) fn native_session_dir() -> Option<String> {
    let root = config_dir()?;
    Some(
        pi_family::sessions::session_directory(&root, &super::sessions::SPEC)
            .to_string_lossy()
            .into_owned(),
    )
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    pi_family::settings_path(root)
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    pi_family::models_path(root)
}

/// Standalone settings/models files for a temporary process; the native agent
/// folder is never read or written.
pub(super) fn temporary_settings(model: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "defaultProvider": PROVIDER_ID,
        "defaultModel": model,
    }))
    .map_err(|err| format!("生成 OmO 临时设置失败: {err}"))
}

pub(super) fn temporary_models(
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "providers": {
            PROVIDER_ID: {
                "name": "BalanceHub",
                "baseUrl": super::super::config_support::openai_base_url(base_url),
                "api": "openai-completions",
                "apiKey": api_key,
                "models": [{ "id": model, "name": model }],
            }
        },
    }))
    .map_err(|err| format!("生成 OmO 临时模型配置失败: {err}"))
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
