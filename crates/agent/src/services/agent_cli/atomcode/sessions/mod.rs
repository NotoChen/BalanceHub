//! AtomCode native session history.
//!
//! AtomCode 5 stores one session per `sessions/<project-hash>/<id>` pair: a
//! `.index` JSON document with metadata and a `.events` NDJSON event log. The
//! summary comes from the index; the transcript streams the event log without
//! loading the whole session.

mod decoder;

#[cfg(test)]
mod tests;

use super::config;
use crate::{
    models::{AgentCliKind, CliSessionSummary},
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
        SessionTranscriptSource,
    },
    services::cli_sessions::{
        clean_text, first_non_empty, read_session_header, timestamp_from_unix,
    },
};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

const PREVIEW_BYTES: usize = 64 * 1024;
const INDEX_PARSER_VERSION: u32 = 2;
const MAX_SESSION_FILES: usize = 100_000;

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("AtomCode 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("ATOMCODE_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_dir().ok_or_else(|| "无法定位 AtomCode 配置目录".to_string())?;
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
    let mut files = Vec::new();
    if let Err(error) = collect(
        &root.join("sessions"),
        budget,
        &mut result.diagnostics,
        &mut files,
    ) {
        result.failed(error);
        return Ok(result);
    }
    files.sort();
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    for path in files {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let record = match mode {
            SessionHistoryReadMode::Identities => identity_record(kind, &path, fallback_workdir),
            SessionHistoryReadMode::Summaries => {
                summary_record(kind, &path, fallback_workdir, budget)
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
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: INDEX_PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: true,
        sqlite: None,
        decode: decoder::decode,
    }])
}

fn collect(
    sessions: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(sessions) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("AtomCode 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("AtomCode 会话目录不是普通目录".into());
    }
    let mut stack = vec![(sessions.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        budget.check()?;
        if depth > 2 {
            diagnostics.push("AtomCode 会话目录层级过深，已跳过部分记录".into());
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(format!("读取 AtomCode 会话目录失败：{error}"));
                continue;
            }
        };
        for entry in entries {
            budget.check()?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    diagnostics.push(error.to_string());
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    diagnostics.push(error.to_string());
                    continue;
                }
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if file_type.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "index")
            {
                if out.len() >= MAX_SESSION_FILES {
                    diagnostics.push("AtomCode 会话文件数量超过上限，已跳过部分记录".into());
                    return Ok(());
                }
                out.push(entry.path());
            }
        }
    }
    Ok(())
}

fn identity_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(path) else {
        return Ok(None);
    };
    let Some(id) = native_session_id(&header, path) else {
        return Ok(None);
    };
    let workdir = header_workdir(&header, fallback_workdir);
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id,
        &workdir,
        events_path(path),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
    budget: &SessionReadBudget,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(path) else {
        return Ok(None);
    };
    let Some(id) = native_session_id(&header, path) else {
        return Ok(None);
    };
    let workdir = header_workdir(&header, fallback_workdir);
    let events = events_path(path);
    let preview = read_first_user_message(&events, budget).and_then(|value| clean_text(value, 240));
    let name = header
        .get("name")
        .and_then(Value::as_str)
        .and_then(|value| clean_text(value, 100));
    let title = first_non_empty([name, preview.clone()]);
    let file_timestamp = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|time| time.to_rfc3339());
    let content_unavailable_reason =
        (!readable_events(&events)).then(|| "AtomCode 会话事件记录缺失".to_string());
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview,
            model: None,
            models: Vec::new(),
            cli_kind: kind,
            created_at: header
                .get("created_at")
                .and_then(Value::as_i64)
                .and_then(|value| timestamp_from_unix(Some(value)))
                .or_else(|| file_timestamp.clone()),
            updated_at: header
                .get("updated_at")
                .and_then(Value::as_i64)
                .and_then(|value| timestamp_from_unix(Some(value)))
                .or(file_timestamp),
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: "atomcodeSession".into(),
        },
        locator: events,
        record_key: id,
        role: crate::models::AgentSessionRole::Main,
        parent_native_id: None,
        resume_reason: None,
        content_unavailable_reason,
    }))
}

fn events_path(index: &Path) -> PathBuf {
    index.with_extension("events")
}

fn readable_events(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_file() && !metadata.file_type().is_symlink())
}

/// Read only the head of the event log until the first user message. The
/// header and the first turn are at the top of the file, so this stays cheap
/// even for very long sessions.
fn read_first_user_message(path: &Path, budget: &SessionReadBudget) -> Option<String> {
    budget.check().ok()?;
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::with_capacity(4096, file.take(PREVIEW_BYTES as u64));
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).ok()?;
        if read == 0 {
            return None;
        }
        budget.record(read).ok()?;
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if value.pointer("/event/kind").and_then(Value::as_str) != Some("user_message") {
            continue;
        }
        return value
            .pointer("/event/text")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
}

fn read_header(path: &Path) -> Option<serde_json::Map<String, Value>> {
    read_session_header(
        path,
        &["id", "name", "working_dir", "created_at", "updated_at"],
        |fields, _| fields.get("id").is_some() && fields.get("updated_at").is_some(),
    )
    .ok()
    .flatten()
}

fn native_session_id(header: &serde_json::Map<String, Value>, path: &Path) -> Option<String> {
    string_field(header, "id").or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .map(str::to_owned)
    })
}

fn header_workdir(header: &serde_json::Map<String, Value>, fallback: Option<&Path>) -> PathBuf {
    header
        .get("working_dir")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

fn string_field(value: &serde_json::Map<String, Value>, name: &str) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
