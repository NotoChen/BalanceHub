//! Factory Droid native session history.
//!
//! Droid keeps one JSONL transcript per session below
//! `<FACTORY_HOME>/.factory/sessions/<cwd-key>/<session-id>.jsonl`, one
//! directory per working directory. The first record is `session_start`
//! (id/title/cwd/parent); later records are messages and compaction states.
//! Rewinds and branches append records linked by `parentId`.

mod decoder;

#[cfg(test)]
mod tests;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
            SessionTranscriptSource,
        },
        cli_sessions::{
            clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited,
        },
    },
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const PARSER_VERSION: u32 = 1;
const METADATA_SOURCE: &str = "droidSessionFiles";
const MAX_SESSION_FILES: usize = 100_000;
const MAX_SCAN_BYTES: usize = 2 * 1024 * 1024;

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Droid 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    if let Some(parent) = root.parent() {
        environment.set_path("FACTORY_HOME_OVERRIDE", parent);
    }
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let home = crate::services::cli_paths::user_home()
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let root = config::config_dir_for_home(&home);
    let (launch_environment, resume_reason) = match launch_environment(&root) {
        Ok(environment) => (environment, None),
        Err(reason) => (EnvironmentPatch::default(), Some(reason.to_owned())),
    };
    Ok(SessionHistorySource {
        config_root: root,
        launch_environment,
        resume_reason,
    })
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let sessions = root.join("sessions");
    let directories = match fs::read_dir(&sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("Droid 会话目录不可读：{error}")),
    };
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut seen = 0usize;
    for directory in directories {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let Ok(directory) = directory else {
            continue;
        };
        let Ok(file_type) = directory.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(directory.path()) {
            Ok(entries) => entries,
            Err(error) => {
                result.failed(format!("Droid 会话目录不可读：{error}"));
                continue;
            }
        };
        for entry in entries {
            if let Err(error) = budget.check() {
                result.failed(error);
                return Ok(result);
            }
            let Ok(entry) = entry else {
                continue;
            };
            seen = seen.saturating_add(1);
            if seen > MAX_SESSION_FILES {
                result.failed("Droid 会话文件数量超过读取上限");
                return Ok(result);
            }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let record = match mode {
                SessionHistoryReadMode::Identities => {
                    identity_record(kind, &path, fallback_workdir)
                }
                SessionHistoryReadMode::Summaries => {
                    summary_record(kind, &path, fallback_workdir)
                }
            };
            match record {
                Ok(Some(record)) => {
                    let workdir = PathBuf::from(&record.summary.workdir);
                    result.workspace(&workdir).records.push(record);
                }
                Ok(None) => {}
                Err(error) => result.failed(error),
            }
        }
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Droid 会话文件不存在".into());
    }
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: true,
        sqlite: None,
        decode: decoder::decode,
    }])
}

#[derive(Default)]
struct SessionFacts {
    start: Option<Value>,
    preview: Option<String>,
    models: BTreeSet<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    message_model: Option<String>,
    latest_millis: i64,
}

fn identity_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let facts = read_facts(path)?;
    let Some(start) = facts.start else {
        return Ok(None);
    };
    let id = session_id(&start, path);
    let workdir = session_workdir(&start, fallback_workdir).unwrap_or_default();
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id,
        &workdir,
        path.to_path_buf(),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let facts = read_facts(path)?;
    let Some(start) = facts.start else {
        return Ok(None);
    };
    let id = session_id(&start, path);
    let workdir = session_workdir(&start, fallback_workdir).unwrap_or_default();
    let preview = facts.preview.clone();
    let title = first_non_empty([
        text(&start, "title").and_then(|value| clean_text(value, 100)),
        preview.clone().and_then(|value| clean_text(value, 100)),
        workdir
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .and_then(|value| clean_text(value, 100)),
    ]);
    let updated_at = facts.updated_at.clone().or_else(|| file_timestamp(path));
    let parent = text(&start, "parent")
        .or_else(|| text(&start, "callingSessionId"))
        .or_else(|| text(&start, "calling_session_id"));
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview,
            model: facts.message_model.clone(),
            models: facts.models.into_iter().collect(),
            cli_kind: kind,
            created_at: facts.created_at,
            updated_at,
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: METADATA_SOURCE.into(),
        },
        locator: path.to_path_buf(),
        record_key: id,
        role: if parent.is_some() {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: parent,
        resume_reason: None,
        content_unavailable_reason: None,
    }))
}

fn read_facts(path: &Path) -> Result<SessionFacts, String> {
    let mut facts = SessionFacts::default();
    read_json_lines_limited(
        path,
        MAX_SCAN_BYTES,
        "读取 Droid 会话失败",
        |_, value| {
            match value.get("type").and_then(Value::as_str) {
                Some("session_start") => {
                    facts.start = Some(value);
                    return;
                }
                Some("compaction_state") => return,
                Some("message") => {}
                _ => return,
            }
            let message = value.get("message").unwrap_or(&Value::Null);
            if message
                .get("visibility")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "llm_only")
            {
                return;
            }
            let timestamp = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|value| normalize_timestamp(Some(value)));
            if let Some(timestamp) = timestamp {
                let millis = chrono::DateTime::parse_from_rfc3339(&timestamp)
                    .ok()
                    .map(|value| value.timestamp_millis())
                    .unwrap_or(i64::MIN);
                if millis >= facts.latest_millis {
                    facts.latest_millis = millis;
                    facts.updated_at = Some(timestamp.clone());
                }
                if facts.created_at.is_none() {
                    facts.created_at = Some(timestamp);
                }
            }
            let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
            if role == "assistant" {
                if let Some(model) = message
                    .get("modelId")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    facts.models.insert(model.to_owned());
                    facts.message_model = Some(model.to_owned());
                }
            }
            if facts.preview.is_none() && role == "user" {
                facts.preview = message
                    .get("content")
                    .and_then(Value::as_array)
                    .and_then(|blocks| {
                        blocks.iter().find_map(|block| {
                            (block.get("type").and_then(Value::as_str) == Some("text"))
                                .then(|| block.get("text").and_then(Value::as_str))
                                .flatten()
                        })
                    })
                    .and_then(|value| clean_text(value, 240));
            }
        },
    )?;
    Ok(facts)
}

fn session_id(start: &Value, path: &Path) -> String {
    text(start, "id").unwrap_or_else(|| {
        path.file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

fn session_workdir(start: &Value, fallback: Option<&Path>) -> Option<PathBuf> {
    text(start, "cwd")
        .or_else(|| text(start, "lastCwd"))
        .or_else(|| text(start, "last_cwd"))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn file_timestamp(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let value = chrono::DateTime::<chrono::Utc>::from(modified);
    Some(value.to_rfc3339())
}
