use super::super::pi_family::{self, FamilySpec};
use crate::models::{AgentCliKind, CliConfigSnapshot, Provider};
use crate::services::agent_cli::configuration::contracts::ProviderConfigurationCandidate;
use crate::services::cli_paths::{configured_path, user_home};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

pub(super) const SPEC: FamilySpec = FamilySpec {
    label: "Pi",
    dir_for_home: config_dir_for_home,
    settings_schema: Some("https://pi.dev/schemas/settings.schema.json"),
    models_schema: Some("https://pi.dev/schemas/models.schema.json"),
};

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    configured_path("PI_CODING_AGENT_DIR").unwrap_or_else(|| home.join(".pi/agent"))
}

fn config_dir() -> Option<PathBuf> {
    configured_path("PI_CODING_AGENT_DIR").or_else(|| user_home().map(|home| home.join(".pi/agent")))
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

/// Pi 的原生会话目录。隔离 `PI_CODING_AGENT_DIR` 后，显式 `--session-dir`
/// 让会话继续写入用户原来的位置；相对路径交给 Pi 按工作目录解析。
pub(super) fn native_session_dir() -> Option<String> {
    if let Some(path) = configured_path("PI_CODING_AGENT_SESSION_DIR") {
        return Some(path.to_string_lossy().into_owned());
    }
    let root = config_dir()?;
    if let Some(configured) = pi_family::read_optional_json(&pi_family::settings_path(&root))
        .and_then(|value| {
            value
                .get("sessionDir")
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    {
        return Some(configured);
    }
    Some(root.join("sessions").to_string_lossy().into_owned())
}
