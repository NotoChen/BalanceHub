use super::config;
use crate::models::AgentCliKind;
use crate::services::agent_cli::{
    contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptSource,
    },
    pi_family::sessions as family,
};
use std::path::{Path, PathBuf};

pub(super) const SPEC: family::SessionRootSpec = family::SessionRootSpec {
    env_keys: &["PI_CODING_AGENT_SESSION_DIR"],
    session_subdir: "sessions",
    settings_session_dir: true,
    launch_env_key: "PI_CODING_AGENT_DIR",
    launch_env_subdir: "",
    metadata_source: "piTranscript",
    resumable: true,
};

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    family::launch_environment(root, &SPEC)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_dir().ok_or_else(|| "无法定位 Pi 原生目录".to_string())?;
    family::source(root, &SPEC)
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    family::scan(kind, root, workdirs, budget, mode, &SPEC)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    family::transcript(record)
}
