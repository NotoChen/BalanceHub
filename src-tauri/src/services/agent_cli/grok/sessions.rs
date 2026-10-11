use crate::{
    limits,
    models::{AgentCliKind, CliSessionMessageRole, CliSessionSummary},
    services::cli_sessions::workbench::{
        read_session_metadata_text_file_limited,
        read_session_text_file_limited as read_text_file_limited,
    },
    services::cli_sessions::{
        clean_text, first_non_empty, normalize_timestamp, timestamp_from_unix,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use super::super::contracts::{
    SessionIndexFormat, SessionIndexMessage, SessionIndexMutation, SessionIndexSource,
    SessionMetadataCursor, SessionMetadataLookupError, SessionMetadataLookupRequest,
    SessionMetadataLookupResult, SessionMetadataSnapshot,
};

const MAX_SUMMARY_FILE_BYTES: usize = 256 * 1024;
const INDEX_PARSER_VERSION: u32 = 2;
mod decoder;
const METADATA_PARSER_VERSION: u32 = 1;
const MAX_METADATA_WORKSPACE_ROOTS: usize = 64;
const MAX_CWD_MARKER_BYTES: u64 = 4 * 1024;

mod history;
pub(super) use history::HISTORY;

/// Reads the bounded Grok summary for one exact workspace and session. The
/// only fallback inspects direct workspace `.cwd` markers for long path names;
/// it never recursively scans historical session summaries.
pub(super) fn metadata_lookup(
    request: SessionMetadataLookupRequest<'_>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    let home = super::config::config_dir()
        .ok_or_else(|| SessionMetadataLookupError::Io("无法定位用户目录".to_string()))?;
    metadata_lookup_from_home(&home, request)
}

fn metadata_lookup_from_home(
    home: &Path,
    request: SessionMetadataLookupRequest<'_>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    let Some(workdir) = request.workdir else {
        return Ok(SessionMetadataLookupResult::NotReady);
    };
    if !is_safe_session_id(request.session_id) {
        return Err(SessionMetadataLookupError::InvalidSource);
    }
    let root = home.join("sessions");
    let Ok(canonical_root) = root.canonicalize() else {
        return Ok(SessionMetadataLookupResult::NotReady);
    };
    let mut candidates = Vec::new();
    if let Some(hint) = request.transcript_path_hint {
        if fs::symlink_metadata(hint)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(SessionMetadataLookupError::InvalidSource);
        }
        let Ok(canonical_hint) = hint.canonicalize() else {
            return Ok(SessionMetadataLookupResult::NotReady);
        };
        if path_has_symlink_component(&canonical_root, hint)?
            || !canonical_hint.starts_with(&canonical_root)
            || canonical_hint
                .file_name()
                .is_none_or(|name| name != "summary.json")
        {
            return Err(SessionMetadataLookupError::InvalidSource);
        }
        candidates.push(canonical_hint);
    }
    if candidates.is_empty() {
        for workspace_root in
            exact_workspace_roots(&canonical_root, workdir, &request.budget, None)?
        {
            let candidate = workspace_root.join(request.session_id).join("summary.json");
            if let Some(candidate) = contained_regular_file(&canonical_root, &candidate)? {
                candidates.push(candidate);
            }
        }
    }
    for path in candidates {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return Ok(SessionMetadataLookupResult::NotReady);
        };
        if !metadata.file_type().is_file() {
            return Err(SessionMetadataLookupError::InvalidSource);
        }
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if length > request.budget.max_bytes {
            return Ok(SessionMetadataLookupResult::Pending {
                partial: None,
                cursor: SessionMetadataCursor {
                    source_identity: path.to_string_lossy().to_string(),
                    source_len: metadata.len(),
                    next_offset: 0,
                    parser_version: METADATA_PARSER_VERSION,
                    opaque_state: Vec::new(),
                },
            });
        }
        request.budget.check(length)?;
        let summary = parse_summary(request.cli_kind, &path, workdir)
            .map_err(SessionMetadataLookupError::Parse)?;
        let Some((summary, _)) = summary.filter(|(summary, _)| summary.id == request.session_id)
        else {
            continue;
        };
        let revision = metadata_revision(&path, &summary);
        let activity = summary
            .updated_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp_millis());
        let snapshot = SessionMetadataSnapshot {
            title: Some(summary.title),
            model: summary.model,
            workdir: Some(summary.workdir),
            last_activity_at: activity,
            source_revision: revision.clone(),
        };
        return Ok(SessionMetadataLookupResult::Ready {
            snapshot,
            cursor: Some(SessionMetadataCursor {
                source_identity: revision,
                source_len: metadata.len(),
                next_offset: metadata.len(),
                parser_version: METADATA_PARSER_VERSION,
                opaque_state: Vec::new(),
            }),
        });
    }
    Ok(SessionMetadataLookupResult::NotReady)
}

fn is_safe_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id != "."
        && session_id != ".."
        && !session_id
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':' | '\0') || character.is_control())
}

fn exact_workspace_roots(
    sessions_root: &Path,
    workdir: &Path,
    budget: &super::super::contracts::SessionMetadataLookupBudget,
    known_markers: Option<&[(PathBuf, String)]>,
) -> Result<Vec<PathBuf>, SessionMetadataLookupError> {
    let mut roots = BTreeSet::new();
    for path in [Some(workdir.to_path_buf()), workdir.canonicalize().ok()]
        .into_iter()
        .flatten()
    {
        let candidate = sessions_root.join(percent_encode_workspace(&path));
        if let Some(candidate) = contained_directory(sessions_root, &candidate)? {
            roots.insert(candidate);
        }
    }

    let local_markers;
    let markers = if let Some(markers) = known_markers {
        markers
    } else {
        local_markers =
            workspace_markers(sessions_root, budget, Some(MAX_METADATA_WORKSPACE_ROOTS))?;
        &local_markers
    };
    for (directory, marker) in markers {
        budget.check(0)?;
        if path_key(Path::new(marker)) == path_key(workdir) {
            if let Some(directory) = contained_directory(sessions_root, directory)? {
                roots.insert(directory);
            }
        }
    }
    Ok(roots.into_iter().collect())
}

fn workspace_markers(
    sessions_root: &Path,
    budget: &super::super::contracts::SessionMetadataLookupBudget,
    max_directories: Option<usize>,
) -> Result<Vec<(PathBuf, String)>, SessionMetadataLookupError> {
    let mut result = Vec::new();
    let entries = fs::read_dir(sessions_root)
        .map_err(|error| SessionMetadataLookupError::Io(error.to_string()))?;
    for (checked, entry) in entries.enumerate() {
        budget.check(0)?;
        if max_directories.is_some_and(|limit| checked >= limit) {
            break;
        }
        let entry = entry.map_err(|error| SessionMetadataLookupError::Io(error.to_string()))?;
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let directory = entry.path();
        let marker = directory.join(".cwd");
        let metadata = match fs::symlink_metadata(&marker) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(SessionMetadataLookupError::Io(error.to_string())),
        };
        if !metadata.is_file() || metadata.len() > MAX_CWD_MARKER_BYTES {
            continue;
        }
        let value = read_session_metadata_text_file_limited(
            &marker,
            MAX_CWD_MARKER_BYTES as usize,
            "读取 Grok 项目目录标记",
        )
        .map_err(SessionMetadataLookupError::Io)?;
        result.push((directory, value.trim().to_string()));
    }
    Ok(result)
}

fn contained_directory(
    root: &Path,
    candidate: &Path,
) -> Result<Option<PathBuf>, SessionMetadataLookupError> {
    let Ok(metadata) = fs::symlink_metadata(candidate) else {
        return Ok(None);
    };
    if path_has_symlink_component(root, candidate)? || !metadata.file_type().is_dir() {
        return Err(SessionMetadataLookupError::InvalidSource);
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|error| SessionMetadataLookupError::Io(error.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(SessionMetadataLookupError::InvalidSource);
    }
    Ok(Some(canonical))
}

fn contained_regular_file(
    root: &Path,
    candidate: &Path,
) -> Result<Option<PathBuf>, SessionMetadataLookupError> {
    let Ok(metadata) = fs::symlink_metadata(candidate) else {
        return Ok(None);
    };
    if path_has_symlink_component(root, candidate)? || !metadata.file_type().is_file() {
        return Err(SessionMetadataLookupError::InvalidSource);
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|error| SessionMetadataLookupError::Io(error.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(SessionMetadataLookupError::InvalidSource);
    }
    Ok(Some(canonical))
}

fn path_has_symlink_component(
    root: &Path,
    candidate: &Path,
) -> Result<bool, SessionMetadataLookupError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| SessionMetadataLookupError::InvalidSource)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| SessionMetadataLookupError::Io(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn percent_encode_workspace(path: &Path) -> String {
    let mut encoded = String::new();
    for byte in path.to_string_lossy().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn metadata_revision(path: &Path, summary: &CliSessionSummary) -> String {
    use sha2::{Digest, Sha256};
    let modified = fs::metadata(path)
        .ok()
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

fn content_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter(|part| !content_part_is_hidden(part))
                .filter_map(|part| {
                    part.as_str().or_else(|| {
                        part.get("text")
                            .or_else(|| part.get("content"))
                            .and_then(Value::as_str)
                    })
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        Value::Object(_) => value
            .get("text")
            .or_else(|| value.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn content_part_is_hidden(value: &Value) -> bool {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["tool", "thought", "thinking", "reasoning", "analysis"]
        .iter()
        .any(|hidden| kind.contains(hidden))
}

fn parse_summary(
    cli_kind: AgentCliKind,
    path: &Path,
    expected_workdir: &Path,
) -> Result<Option<(CliSessionSummary, bool)>, String> {
    let text = read_text_file_limited(
        path,
        MAX_SUMMARY_FILE_BYTES.min(limits::MAX_CLI_CONFIG_FILE_BYTES),
        "读取 Grok Build 会话摘要",
    )?;
    let value = serde_json::from_str::<Value>(&text)
        .map_err(|err| format!("解析 Grok Build 会话摘要失败({}): {err}", path.display()))?;
    let Some(info) = value.get("info").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(id) = info
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    let Some(workdir) = info
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|workdir| !workdir.is_empty())
    else {
        return Ok(None);
    };
    if path_key(Path::new(workdir)) != path_key(expected_workdir) {
        return Ok(None);
    }
    let is_subagent = value
        .get("session_kind")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.starts_with("subagent"));
    if value.get("hidden").and_then(Value::as_bool) == Some(true) && !is_subagent {
        return Ok(None);
    }
    let has_message_count =
        value.get("num_messages").is_some() || value.get("num_chat_messages").is_some();
    let num_messages = value
        .get("num_messages")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let num_chat_messages = value
        .get("num_chat_messages")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if has_message_count && num_messages == 0 && num_chat_messages == 0 {
        return Ok(None);
    }

    let generated_title = string_field(&value, "generated_title");
    let session_summary = string_field(&value, "session_summary");
    let last_turn_summary = string_field(&value, "last_turn_summary");
    let title = first_non_empty([
        generated_title
            .as_deref()
            .and_then(|title| clean_text(title, 100)),
        session_summary
            .as_deref()
            .and_then(|title| clean_text(title, 100)),
        last_turn_summary
            .as_deref()
            .and_then(|title| clean_text(title, 100)),
    ]);
    let preview = session_summary
        .as_deref()
        .filter(|summary| generated_title.as_deref() != Some(*summary))
        .and_then(|summary| clean_text(summary, 240))
        .or_else(|| last_turn_summary.and_then(|summary| clean_text(summary, 240)));
    let model = string_field(&value, "current_model_id").and_then(|model| clean_text(model, 120));
    let models = model
        .clone()
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let created_at = normalize_timestamp(value.get("created_at").and_then(Value::as_str));
    let updated_at = normalize_timestamp(value.get("last_active_at").and_then(Value::as_str))
        .or_else(|| normalize_timestamp(value.get("updated_at").and_then(Value::as_str)));

    Ok(Some((
        CliSessionSummary {
            id: id.to_string(),
            title,
            preview,
            model,
            models,
            cli_kind,
            created_at,
            updated_at,
            workdir: workdir.to_string(),
            cli_version: None,
            archived: false,
            can_resume: !is_subagent,
            metadata_source: "grokSummary".to_string(),
        },
        is_subagent,
    )))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn path_key(path: &Path) -> String {
    let mut value = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        value.make_ascii_lowercase();
    }
    value
}

#[cfg(test)]
mod tests;
