//! StepCode native session history.
//!
//! StepCode persists the same append-only Pi entry model with one different
//! root: sessions live below `<config>/agent/sessions`. Reuse the shared
//! Pi-family scanner and decoder instead of maintaining a second copy.

use super::super::pi_family::sessions as family;
use super::config;
use crate::{
    models::AgentCliKind,
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptSource,
    },
};
use std::path::{Path, PathBuf};

pub(super) const SPEC: family::SessionRootSpec = family::SessionRootSpec {
    env_keys: &["STEP_CODING_AGENT_SESSION_DIR"],
    session_subdir: "agent/sessions",
    settings_session_dir: false,
    launch_env_key: "STEP_CODING_AGENT_DIR",
    launch_env_subdir: "agent",
    metadata_source: "stepTranscript",
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
    let root = config::config_dir().ok_or_else(|| "无法定位 StepCode 配置目录".to_string())?;
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
