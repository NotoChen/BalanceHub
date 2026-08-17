mod config;
mod launch;
mod liveness;
mod sessions;

use super::{
    contracts::{
        DefaultConfigAdapter, EndpointAdapter, LivenessAdapter, SessionAdapter,
        TemporaryLaunchAdapter, TemporaryLaunchFeatures,
    },
    AgentCliDefinition,
};
use super::discovery::paths::node_cli_home_candidates;
use crate::models::{AgentCliKind, CliSessionSummary};
use std::path::{Path, PathBuf};

pub(super) const fn definition(kind: AgentCliKind) -> AgentCliDefinition {
    AgentCliDefinition {
        kind,
        label: "Codex CLI",
        executable: "codex",
        session_name_hint: "Codex CLI 当前不支持启动前命名；启动后可在终端输入 /new 名称 或 /rename",
        additional_env_keys: &["CODEX_CLI_PATH"],
        home_candidates,
        invalid_path_reason: Some(invalid_path_reason),
        require_version_substring: None,
        endpoint: EndpointAdapter::new(normalize_base_url),
        temporary_launch: Some(TemporaryLaunchAdapter::new(
            TemporaryLaunchFeatures {
                model_selection: true,
                session_resume: true,
                session_name: false,
            },
            None,
            launch::build_plan,
        )),
        sessions: Some(SessionAdapter::new(list_sessions)),
        liveness: Some(LivenessAdapter::new(
            liveness::build_plan,
            liveness::parse_output,
        )),
        default_config: Some(DefaultConfigAdapter::new(
            config::snapshot,
            config::preview,
            config::switch,
        )),
    }
}

fn normalize_base_url(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return normalized;
    }
    if normalized.ends_with("/v1") {
        normalized
    } else {
        format!("{normalized}/v1")
    }
}

fn list_sessions(
    cli_kind: AgentCliKind,
    workdir: &Path,
) -> Result<Vec<CliSessionSummary>, String> {
    sessions::list(cli_kind, workdir, 100)
}

fn home_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = node_cli_home_candidates(home, "codex");
    candidates.push(home.join(".codex/bin/codex"));
    candidates
}

fn invalid_path_reason(path: &Path) -> Option<&'static str> {
    let value = path.to_string_lossy().replace('\\', "/");
    value
        .contains(".app/Contents/")
        .then_some("不支持使用 Codex Desktop App 内置二进制，请安装并选择独立的 codex CLI")
}
