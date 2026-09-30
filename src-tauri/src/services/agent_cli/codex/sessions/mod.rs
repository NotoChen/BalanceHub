use crate::{
    models::{AgentCliKind, CliSessionDetail, CliSessionSummary},
    services::cli_sessions::clean_text,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::super::contracts::{
    SessionContentSearchRequest, SessionContentSearchResult, SessionIndexLoadResult,
    SessionMetadataCursor, SessionMetadataLookupError, SessionMetadataLookupRequest,
    SessionMetadataLookupResult, SessionMetadataSnapshot, SessionReadLimits,
};

const METADATA_PARSER_VERSION: u32 = 1;
const MAX_METADATA_DATABASES: usize = 8;

mod index;
mod rollout;

use index::{read_database_session, read_session_title, state_databases};
use rollout::{index_rollout, parse_rollout_messages, search_rollout};

mod history;
pub(super) use history::HISTORY;

/// Reads one exact Codex thread row. This intentionally does not call `list`
/// and never walks the rollout tree; the state database is the authoritative
/// identity index for a running session.
pub(super) fn metadata_lookup(
    request: SessionMetadataLookupRequest<'_>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    let Some(workdir) = request.workdir else {
        return Ok(SessionMetadataLookupResult::NotReady);
    };
    request.budget.check(64 * 1024)?;
    let _previous_cursor = request.previous;
    let home = codex_home().map_err(SessionMetadataLookupError::Io)?;
    let databases = state_databases(&home).map_err(SessionMetadataLookupError::Io)?;
    let workdir_text = workdir.to_string_lossy().to_string();
    let canonical_workdir = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .to_string_lossy()
        .to_string();
    for database in databases.into_iter().take(MAX_METADATA_DATABASES) {
        if let Some(mut record) = read_database_session(
            request.cli_kind,
            &database,
            &workdir_text,
            &canonical_workdir,
            request.session_id,
        )
        .map_err(SessionMetadataLookupError::Io)?
        {
            if let Some(title) =
                read_session_title(&home, request.session_id, request.budget.max_bytes)
            {
                record.summary.title = title;
            }
            let revision = metadata_revision(&database, &record.summary);
            let snapshot = SessionMetadataSnapshot {
                title: Some(record.summary.title),
                model: record.summary.model,
                workdir: Some(record.summary.workdir),
                last_activity_at: record
                    .summary
                    .updated_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.timestamp_millis()),
                source_revision: revision.clone(),
            };
            return Ok(SessionMetadataLookupResult::Ready {
                snapshot,
                cursor: Some(SessionMetadataCursor {
                    source_identity: revision,
                    source_len: fs::metadata(&database).map(|m| m.len()).unwrap_or_default(),
                    next_offset: 0,
                    parser_version: METADATA_PARSER_VERSION,
                    opaque_state: Vec::new(),
                }),
            });
        }
    }
    Ok(SessionMetadataLookupResult::NotReady)
}

fn metadata_revision(path: &Path, summary: &CliSessionSummary) -> String {
    use sha2::{Digest, Sha256};
    let metadata = fs::metadata(path).ok();
    let modified = metadata
        .and_then(|value| value.modified().ok())
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos().to_string())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(modified.as_bytes());
    hasher.update(summary.id.as_bytes());
    hasher.update(summary.title.as_bytes());
    hasher.update(summary.model.as_deref().unwrap_or_default().as_bytes());
    hasher.update(summary.updated_at.as_deref().unwrap_or_default().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn codex_home() -> Result<PathBuf, String> {
    super::config::config_dir()
        .ok_or_else(|| "无法定位用户目录，无法读取 Codex 历史会话".to_string())
}

#[cfg(test)]
mod tests;
