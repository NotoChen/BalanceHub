use super::super::pi_family::{self, FamilySpec};
use crate::models::{AgentCliKind, CliConfigSnapshot, Provider};
use crate::services::agent_cli::configuration::contracts::ProviderConfigurationCandidate;
use crate::services::cli_paths::{configured_path, user_home};
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

pub(super) fn config_dir() -> Option<PathBuf> {
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
/// 让会话继续写入用户原来的位置；目录解析与原生会话扫描共用同一套规则，
/// 相对路径的覆盖会被忽略，不会按项目工作目录重新落点。
pub(super) fn native_session_dir() -> Option<String> {
    let root = config_dir()?;
    Some(
        pi_family::sessions::session_directory(&root, &super::sessions::SPEC)
            .to_string_lossy()
            .into_owned(),
    )
}
