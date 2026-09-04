use crate::{
    models::{AgentCliKind, CliSessionDetail, CliSessionSummary},
    services::cli_sessions::clean_text,
};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use super::super::contracts::{
    SessionContentSearchRequest, SessionContentSearchResult, SessionIndexLoadResult,
    SessionMetadataLookupError, SessionMetadataLookupRequest, SessionMetadataLookupResult,
    SessionMetadataSnapshot, SessionMetadataCursor, SessionReadLimits,
};

const METADATA_PARSER_VERSION: u32 = 1;
const MAX_METADATA_DATABASES: usize = 8;

mod index;
mod rollout;

use index::{
    read_database, read_database_session, read_session_title, read_session_titles, resolve_rollout_path,
    state_databases,
};
use rollout::{index_rollout, parse_rollout_messages, search_rollout};

pub(super) fn list(
    cli_kind: AgentCliKind,
    workdir: &Path,
    limit: usize,
) -> Result<Vec<CliSessionSummary>, String> {
    let codex_home = codex_home()?;
    // The official resume picker keeps explicit session names separately from
    // the SQLite thread metadata. Read it independently so a missing or stale
    // index never prevents the database-backed history from loading.
    let session_titles = read_session_titles(&codex_home).unwrap_or_default();
    let databases = state_databases(&codex_home)?;
    if databases.is_empty() {
        return Err(format!("未找到 Codex 状态数据库：{}", codex_home.display()));
    }

    let workdir = workdir.to_string_lossy().to_string();
    let canonical_workdir = Path::new(&workdir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&workdir))
        .to_string_lossy()
        .to_string();
    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    let mut last_error = None;
    let mut readable_database = false;
    for database in databases {
        match read_database(cli_kind, &database, &workdir, &canonical_workdir, limit) {
            Ok(items) => {
                readable_database = true;
                for item in items {
                    if seen.insert(item.id.clone()) {
                        sessions.push(item);
                    }
                }
            }
            Err(err) => last_error = Some(err),
        }
    }
    if !readable_database {
        if let Some(err) = last_error {
            return Err(err);
        }
    }
    for session in &mut sessions {
        if let Some(title) = session_titles
            .get(&session.id)
            .and_then(|value| clean_text(value, 100))
        {
            session.title = title;
        }
    }
    Ok(sessions)
}

pub(super) fn detail(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    limits: SessionReadLimits,
) -> Result<CliSessionDetail, String> {
    let codex_home = codex_home()?;
    let workdir_text = workdir.to_string_lossy().to_string();
    let canonical_workdir = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .to_string_lossy()
        .to_string();
    let session_titles = read_session_titles(&codex_home).unwrap_or_default();
    let mut last_error = None;
    let mut record = None;
    for database in state_databases(&codex_home)? {
        match read_database_session(
            cli_kind,
            &database,
            &workdir_text,
            &canonical_workdir,
            session_id,
        ) {
            Ok(Some(value)) => {
                record = Some(value);
                break;
            }
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }
    }
    let Some(mut record) = record else {
        return Err(last_error.unwrap_or_else(|| "未找到指定的 Codex 会话".to_string()));
    };
    if let Some(title) = session_titles
        .get(&record.summary.id)
        .and_then(|value| clean_text(value, 100))
    {
        record.summary.title = title;
    }
    let rollout_path = resolve_rollout_path(&codex_home, &record.rollout_path)
        .ok_or_else(|| "Codex 会话索引存在，但正文文件已不可用".to_string())?;
    let (messages, truncated, omitted_message_count) =
        parse_rollout_messages(&rollout_path, limits)?;
    Ok(CliSessionDetail {
        session: record.summary,
        messages,
        truncated,
        omitted_message_count,
        content_source: "codexRollout".to_string(),
    })
}

pub(super) fn search(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let codex_home = codex_home()?;
    let workdir_text = workdir.to_string_lossy().to_string();
    let canonical_workdir = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut last_error = None;
    for database in state_databases(&codex_home)? {
        match read_database_session(
            cli_kind,
            &database,
            &workdir_text,
            &canonical_workdir,
            session_id,
        ) {
            Ok(Some(record)) => {
                let path = resolve_rollout_path(&codex_home, &record.rollout_path)
                    .ok_or_else(|| "Codex 会话索引存在，但正文文件已不可用".to_string())?;
                return search_rollout(&path, request, is_current);
            }
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "未找到指定的 Codex 会话".to_string()))
}

pub(super) fn index(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
    known_fingerprint: Option<&str>,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String> {
    let codex_home = codex_home()?;
    let workdir_text = workdir.to_string_lossy().to_string();
    let canonical_workdir = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .to_string_lossy()
        .to_string();
    let mut last_error = None;
    for database in state_databases(&codex_home)? {
        match read_database_session(
            cli_kind,
            &database,
            &workdir_text,
            &canonical_workdir,
            session_id,
        ) {
            Ok(Some(record)) => {
                let path = resolve_rollout_path(&codex_home, &record.rollout_path)
                    .ok_or_else(|| "Codex 会话索引存在，但正文文件已不可用".to_string())?;
                return index_rollout(&path, known_fingerprint, is_current);
            }
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "未找到指定的 Codex 会话".to_string()))
}

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
            if let Some(title) = read_session_title(&home, request.session_id, request.budget.max_bytes)
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
