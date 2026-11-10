//! DevEco Code SQLite session history.

use super::super::opencode_family::sessions::{self, SessionSpec};
use crate::services::agent_cli::contracts::{
    EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
    SessionHistoryScan, SessionHistorySource, SessionReadBudget,
};
use std::path::{Path, PathBuf};

const SPEC: SessionSpec = SessionSpec {
    data_dir,
    db_name: "deveco.db",
    db_env: "DEVECO_DB",
    metadata_source: "devecoDatabase",
    resume_reason: None,
};

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn data_dir() -> Option<PathBuf> {
    sessions::app_data_dir("deveco")
}

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    sessions::launch_environment(root)
}

fn source() -> Result<SessionHistorySource, String> {
    sessions::source(&SPEC)
}

fn scan(
    kind: crate::models::AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    sessions::scan(&SPEC, kind, root, workdirs, budget, mode)
}

fn transcript(
    record: &SessionHistoryRecord,
) -> Result<Vec<crate::services::agent_cli::contracts::SessionTranscriptSource>, String> {
    sessions::transcript(record)
}
