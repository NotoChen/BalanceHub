//! OpenClaw transcript history.
//!
//! Current OpenClaw keeps session windows and Pi-format transcript events in
//! `<state>/agents/<agent>/agent/openclaw-agent.sqlite`. Older installs also
//! keep append-only JSONL files below `<state>/agents/<agent>/sessions`; both
//! sources are read so an upgrade does not hide existing history.

mod runtime;

#[cfg(test)]
mod tests;

use super::config;
use crate::{
    models::AgentCliKind,
    services::agent_cli::{
        contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptSource,
        },
        pi_family,
    },
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

const JSONL_SOURCE: &str = "openclawTranscript";

const SPEC: pi_family::sessions::SessionRootSpec = pi_family::sessions::SessionRootSpec {
    env_keys: &[],
    settings_session_dir: false,
    session_subdir: "agents",
    launch_env_key: "OPENCLAW_STATE_DIR",
    launch_env_subdir: "",
    metadata_source: JSONL_SOURCE,
    resumable: false,
};

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    pi_family::sessions::launch_environment(root, &SPEC)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::state_dir().ok_or_else(|| "无法定位 OpenClaw 状态目录".to_string())?;
    pi_family::sessions::source(root, &SPEC)
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = pi_family::sessions::scan(kind, root, workdirs, budget, mode, &SPEC)?;
    let mut runtime_ids = HashSet::new();
    for path in runtime::database_paths(root) {
        if let Err(error) = runtime::scan_database(
            kind,
            &path,
            workdirs,
            budget,
            mode,
            &mut result,
            &mut runtime_ids,
        ) {
            result.failed(format!("OpenClaw 会话数据库读取失败：{error}"));
        }
    }
    if !runtime_ids.is_empty() {
        // The JSONL export and the runtime database can describe the same
        // session. The database copy is authoritative once it exists.
        for workspace in &mut result.workspaces {
            workspace.records.retain(|record| {
                record.summary.metadata_source != JSONL_SOURCE
                    || !runtime_ids.contains(&record.record_key)
            });
        }
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if record.summary.metadata_source == runtime::METADATA_SOURCE {
        return runtime::transcript(record);
    }
    pi_family::sessions::transcript(record)
}
