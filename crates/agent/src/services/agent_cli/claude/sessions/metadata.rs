use super::{
    parse_transcript,
    projects::{ProjectCandidateState, ProjectLookup, ProjectMatch, ProjectSelector},
    ParsedClaudeTranscript,
};
use crate::{
    models::CliSessionSummary,
    services::{
        agent_cli::contracts::{
            SessionMetadataCursor, SessionMetadataLookupError, SessionMetadataLookupRequest,
            SessionMetadataLookupResult, SessionMetadataSnapshot, SessionReadBudget,
        },
        cli_sessions::workbench::{with_read_budget, with_source_root},
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

const METADATA_PARSER_VERSION: u32 = 1;

pub(super) fn lookup(
    root: &Path,
    request: SessionMetadataLookupRequest<'_>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    request.budget.check(0)?;
    let budget = SessionReadBudget::new(
        request.budget.deadline,
        request.budget.cancelled.clone(),
        request.budget.max_bytes as u64,
    );
    with_read_budget(&budget, || {
        with_source_root(root, || lookup_with_budget(root, request, &budget))
    })
}

fn check(
    request: &SessionMetadataLookupRequest<'_>,
    budget: &SessionReadBudget,
) -> Result<(), SessionMetadataLookupError> {
    request
        .budget
        .check(usize::try_from(budget.bytes.load(Ordering::Relaxed)).unwrap_or(usize::MAX))
}

fn lookup_with_budget(
    root: &Path,
    request: SessionMetadataLookupRequest<'_>,
    budget: &SessionReadBudget,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    check(&request, budget)?;
    let Some(workdir) = request.workdir else {
        return Ok(SessionMetadataLookupResult::NotReady);
    };
    let selector = ProjectSelector::new(workdir);
    let explicit_hint = request.transcript_path_hint.is_some();
    let lookup = ProjectLookup::new(root, budget).map_err(|error| {
        check(&request, budget).err().unwrap_or({
            if explicit_hint {
                SessionMetadataLookupError::InvalidSource
            } else {
                SessionMetadataLookupError::Io(error)
            }
        })
    })?;
    let paths: Vec<(PathBuf, ProjectMatch)> = if let Some(hint) = request.transcript_path_hint {
        if hint.extension().is_none_or(|ext| ext != "jsonl") {
            return Err(SessionMetadataLookupError::InvalidSource);
        }
        let parent = hint
            .parent()
            .ok_or(SessionMetadataLookupError::InvalidSource)?;
        let candidate = lookup.candidate(parent, &selector, budget).map_err(|_| {
            check(&request, budget)
                .err()
                .unwrap_or(SessionMetadataLookupError::InvalidSource)
        })?;
        check(&request, budget)?;
        let candidate = match candidate {
            ProjectCandidateState::Ready(candidate) => candidate,
            ProjectCandidateState::Missing => return Ok(SessionMetadataLookupResult::NotReady),
            ProjectCandidateState::Invalid => {
                return Err(SessionMetadataLookupError::InvalidSource)
            }
        };
        let filename = hint
            .file_name()
            .ok_or(SessionMetadataLookupError::InvalidSource)?;
        vec![(candidate.path.join(filename), candidate.kind)]
    } else {
        if request
            .session_id
            .chars()
            .any(|character| matches!(character, '/' | '\\'))
        {
            return Err(SessionMetadataLookupError::InvalidSource);
        }
        lookup
            .discover(std::slice::from_ref(&selector), budget)
            .map_err(|error| {
                check(&request, budget)
                    .err()
                    .unwrap_or(SessionMetadataLookupError::Io(error))
            })?
            .into_iter()
            .flatten()
            .map(|candidate| {
                (
                    candidate.path.join(format!("{}.jsonl", request.session_id)),
                    candidate.kind,
                )
            })
            .collect()
    };
    let mut ready = None;
    let mut pending = None;
    let mut unknown_origin = false;
    for (path, project_match) in paths {
        check(&request, budget)?;
        let (parsed, source_len) = match read_candidate(&path, &request, budget)? {
            MetadataFile::Missing => continue,
            MetadataFile::Pending(cursor) => {
                pending.get_or_insert(cursor);
                continue;
            }
            MetadataFile::Parsed(parsed, source_len) => (parsed, source_len),
        };
        if parsed.summary.id != request.session_id {
            if explicit_hint {
                return Err(SessionMetadataLookupError::InvalidSource);
            }
            continue;
        }
        if project_match == ProjectMatch::Prefix {
            match selector.proves_origin(parsed.native_origin_workdir.as_deref()) {
                Some(true) => {}
                Some(false) if explicit_hint => {
                    return Err(SessionMetadataLookupError::InvalidSource)
                }
                Some(false) => continue,
                None => {
                    unknown_origin = true;
                    continue;
                }
            }
        }
        let mut summary = parsed.summary;
        summary.workdir = workdir.to_string_lossy().to_string();
        check(&request, budget)?;
        if ready.is_some() {
            return Err(SessionMetadataLookupError::InvalidSource);
        }
        ready = Some(ready_result(&path, summary, source_len));
    }
    check(&request, budget)?;
    if unknown_origin {
        Ok(SessionMetadataLookupResult::NotReady)
    } else if let Some(cursor) = pending {
        Ok(SessionMetadataLookupResult::Pending {
            partial: None,
            cursor,
        })
    } else {
        Ok(ready.unwrap_or(SessionMetadataLookupResult::NotReady))
    }
}

enum MetadataFile {
    Missing,
    Pending(SessionMetadataCursor),
    Parsed(Box<ParsedClaudeTranscript>, u64),
}

fn read_candidate(
    path: &Path,
    request: &SessionMetadataLookupRequest<'_>,
    budget: &SessionReadBudget,
) -> Result<MetadataFile, SessionMetadataLookupError> {
    check(request, budget)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MetadataFile::Missing)
        }
        Err(error) => return Err(SessionMetadataLookupError::Io(error.to_string())),
    };
    if !metadata.file_type().is_file() {
        return Err(SessionMetadataLookupError::InvalidSource);
    }
    let pending = || SessionMetadataCursor {
        source_identity: path.to_string_lossy().to_string(),
        source_len: metadata.len(),
        next_offset: 0,
        parser_version: METADATA_PARSER_VERSION,
        opaque_state: Vec::new(),
    };
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if length > request.budget.max_bytes {
        return Ok(MetadataFile::Pending(pending()));
    }
    let used = usize::try_from(budget.bytes.load(Ordering::Relaxed)).unwrap_or(usize::MAX);
    request.budget.check(used.saturating_add(length))?;
    let parsed = parse_transcript(request.cli_kind, path).map_err(|error| {
        check(request, budget)
            .err()
            .unwrap_or(SessionMetadataLookupError::Parse(error))
    })?;
    check(request, budget)?;
    match parsed {
        Some(parsed) => Ok(MetadataFile::Parsed(Box::new(parsed), metadata.len())),
        None => Ok(MetadataFile::Missing),
    }
}

fn ready_result(
    path: &Path,
    summary: CliSessionSummary,
    source_len: u64,
) -> SessionMetadataLookupResult {
    use sha2::{Digest, Sha256};
    let modified = fs::metadata(path)
        .ok()
        .and_then(|value| value.modified().ok())
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos().to_string())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    let path_identity = path.to_string_lossy();
    for value in [
        path_identity.as_ref(),
        &modified,
        &summary.id,
        &summary.title,
        summary.model.as_deref().unwrap_or_default(),
        summary.updated_at.as_deref().unwrap_or_default(),
    ] {
        hasher.update(value.as_bytes());
    }
    let revision = format!("{:x}", hasher.finalize());
    let activity = summary
        .updated_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis());
    SessionMetadataLookupResult::Ready {
        snapshot: SessionMetadataSnapshot {
            title: Some(summary.title),
            model: summary.model,
            workdir: Some(summary.workdir),
            last_activity_at: activity,
            source_revision: revision.clone(),
        },
        cursor: Some(SessionMetadataCursor {
            source_identity: revision,
            source_len,
            next_offset: source_len,
            parser_version: METADATA_PARSER_VERSION,
            opaque_state: Vec::new(),
        }),
    }
}

#[cfg(test)]
mod tests;
