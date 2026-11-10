//! dsh native session log.
//!
//! Sessions live at `<DSH_HOME>/sessions/<slug>/session-<id>/session.v*.jsonl`
//! and may be zstd-compressed. The first line is a `session` header; every
//! following line is a `{type, seq, time, data}` event.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
        SessionTranscriptSource,
    },
    services::cli_sessions::{
        clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited,
        read_session_header, source as session_source,
    },
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const SUMMARY_BYTES: usize = 256 * 1024;
const INDEX_PARSER_VERSION: u32 = 1;
const MAX_DIRECTORY_DEPTH: usize = 6;
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
        return Err("dsh 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("DSH_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::data_dir().ok_or_else(|| "无法定位 dsh 数据目录".to_string())?;
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
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    let path = session_source::existing_variant(&record.locator);
    if !path.is_file() {
        return Err("dsh 会话日志不存在".into());
    }
    Ok(vec![SessionTranscriptSource {
        path,
        parser_version: INDEX_PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: true,
        sqlite: None,
        decode: decoder::decode,
    }])
}

fn collect(
    root: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("dsh 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("dsh 会话目录不是普通目录".into());
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        budget.check()?;
        if depth > MAX_DIRECTORY_DEPTH {
            diagnostics.push("dsh 会话目录层级过深，已跳过部分记录".into());
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(format!("读取 dsh 会话目录失败：{error}"));
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
            } else if file_type.is_file() && is_session_log(&entry.path()) {
                if files.len() >= MAX_SESSION_FILES {
                    diagnostics.push("dsh 会话数量超过上限，已跳过部分记录".into());
                    return Ok(());
                }
                files.push(entry.path());
            }
        }
    }
    Ok(())
}

fn is_session_log(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("session.") && session_source::json_lines(path))
}

fn read_header(path: &Path) -> Option<serde_json::Map<String, Value>> {
    read_session_header(
        path,
        &[
            "type",
            "version",
            "id",
            "createdAt",
            "cwd",
            "parentSession",
            "origin",
            "delegationDepth",
            "agentPreset",
        ],
        |fields, _| {
            fields.get("type").and_then(Value::as_str) == Some("session")
                && fields
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
        },
    )
    .ok()
    .flatten()
}

fn header_id(header: &serde_json::Map<String, Value>, path: &Path) -> Option<String> {
    header
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        })
}

fn workdir(header: &serde_json::Map<String, Value>, fallback: Option<&Path>) -> PathBuf {
    header
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

fn identity_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(path) else {
        return Ok(None);
    };
    let Some(id) = header_id(&header, path) else {
        return Ok(None);
    };
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id,
        &workdir(&header, fallback_workdir),
        path.to_path_buf(),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(path) else {
        return Ok(None);
    };
    let Some(id) = header_id(&header, path) else {
        return Ok(None);
    };
    let mut parsed = FileSummary::default();
    let _ = read_json_lines_limited(path, SUMMARY_BYTES, "读取 dsh 会话摘要", |_, value| {
        parsed.observe(&value)
    })?;
    let workdir = workdir(&header, fallback_workdir);
    let title = first_non_empty([
        parsed.title.and_then(|value| clean_text(value, 100)),
        parsed
            .first_user_text
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = parsed
        .first_user_text
        .and_then(|value| clean_text(value, 240));
    let file_timestamp = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|time| time.to_rfc3339());
    let parent = header
        .get("parentSession")
        .and_then(Value::as_str)
        .map(|value| {
            Path::new(value)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(value)
                .to_owned()
        });
    let subagent = header
        .get("delegationDepth")
        .and_then(Value::as_i64)
        .is_some_and(|depth| depth > 0)
        || parent.is_some();
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview,
            model: parsed.model,
            models: parsed.models.into_iter().collect(),
            cli_kind: kind,
            created_at: header
                .get("createdAt")
                .and_then(Value::as_str)
                .and_then(|value| normalize_timestamp(Some(value)))
                .or_else(|| file_timestamp.clone()),
            updated_at: parsed.updated_at.or(file_timestamp),
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: !subagent,
            metadata_source: "dshSessionLog".into(),
        },
        locator: path.to_path_buf(),
        record_key: id,
        role: if subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: parent,
        resume_reason: subagent.then(|| "dsh 子代理会话不能单独续聊".into()),
        content_unavailable_reason: None,
    }))
}

#[derive(Default)]
struct FileSummary {
    title: Option<String>,
    first_user_text: Option<String>,
    model: Option<String>,
    models: std::collections::BTreeSet<String>,
    updated_at: Option<String>,
}

impl FileSummary {
    fn observe(&mut self, value: &Value) {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
        if let Some(timestamp) = value
            .get("time")
            .and_then(Value::as_str)
            .and_then(|value| normalize_timestamp(Some(value)))
        {
            self.updated_at = Some(timestamp);
        }
        let data = value.get("data").unwrap_or(&Value::Null);
        match kind {
            "session/title" => {
                if let Some(title) = data
                    .get("title")
                    .or_else(|| data.get("name"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    self.title = Some(title.to_owned());
                }
            }
            "user/message" => {
                let message = data.get("message").unwrap_or(data);
                if message.get("role").and_then(Value::as_str) == Some("user")
                    && self.first_user_text.is_none()
                    && message
                        .pointer("/source/kind")
                        .and_then(Value::as_str)
                        .is_none_or(|kind| kind == "user")
                {
                    self.first_user_text = content_text(message.get("content"));
                }
            }
            "assistant/message" => {
                if let Some(model) = data
                    .pointer("/message/source/model")
                    .or_else(|| data.pointer("/message/model"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    self.models.insert(model.to_owned());
                    self.model = Some(model.to_owned());
                }
            }
            _ => {}
        }
    }
}

fn content_text(value: Option<&Value>) -> Option<String> {
    let blocks = value?.as_array()?;
    let text = blocks
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    block
                        .as_str()
                        .map(str::to_owned)
                        .filter(|value| !value.trim().is_empty())
                })
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
