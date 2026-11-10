//! Command Code native session history.
//!
//! v3 transcripts live under `~/.commandcode/projects/<slug>/<id>.jsonl`:
//! the first line is a `session` header, followed by typed entries whose
//! `message` entries carry the conversation. `<id>.meta.json` holds the
//! display title, model and entrypoint. Legacy v1/v2 files are not indexed.

mod decoder;

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
            read_session_header,
            workbench::{
                cached_history_record_facts, read_session_metadata_text_file_limited,
                HistoryRecordFacts,
            },
        },
    },
};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const PARSER_VERSION: u32 = 1;
const METADATA_SOURCE: &str = "commandCodeTranscript";
const SUMMARY_BYTES: usize = 256 * 1024;
const META_BYTES: usize = 16 * 1024;
const MAX_SESSION_FILES: usize = 100_000;
const MAX_DIRECTORY_DEPTH: usize = 2;
const SESSION_VERSION: u64 = 3;

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

/// Command Code has no configuration-directory variable; it derives
/// `~/.commandcode` from `HOME`. The isolation root is therefore the native
/// config directory, and the parent becomes the temporary home directory.
fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Command Code 原生目录必须是绝对路径");
    }
    let home = root
        .parent()
        .ok_or("Command Code 配置目录缺少父目录")?;
    let mut environment = EnvironmentPatch::default();
    environment.set_path("HOME", home);
    environment.set_path("USERPROFILE", home);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_root().ok_or_else(|| "无法定位用户目录".to_string())?;
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
    let mut context = ScanContext {
        kind,
        fallback_workdir: (workdirs.len() == 1).then(|| workdirs[0].as_path()),
        budget,
        mode,
        seen: HashSet::new(),
        files: 0,
        result: SessionHistoryScan::new(&[]),
    };
    context.walk(&root.join("projects"), 0)?;
    Ok(context.result)
}

struct ScanContext<'a> {
    kind: AgentCliKind,
    fallback_workdir: Option<&'a Path>,
    budget: &'a SessionReadBudget,
    mode: SessionHistoryReadMode,
    seen: HashSet<String>,
    files: usize,
    result: SessionHistoryScan,
}

impl ScanContext<'_> {
    fn walk(&mut self, directory: &Path, depth: usize) -> Result<(), String> {
        self.budget.check()?;
        if depth > MAX_DIRECTORY_DEPTH {
            return Ok(());
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("Command Code 会话目录不可读：{error}")),
        };
        for entry in entries {
            self.budget.check()?;
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                self.walk(&path, depth + 1)?;
                continue;
            }
            if !file_type.is_file() || !is_transcript(&path) {
                continue;
            }
            self.files = self.files.saturating_add(1);
            if self.files > MAX_SESSION_FILES {
                self.result
                    .failed("Command Code 会话文件数量超过读取上限");
                return Ok(());
            }
            match add_record(self.kind, &path, self.fallback_workdir, self.mode) {
                Ok(Some(record)) => {
                    if !self.seen.insert(record.record_key.clone()) {
                        continue;
                    }
                    let workdir = PathBuf::from(&record.summary.workdir);
                    self.result.workspace(&workdir).records.push(record);
                }
                Ok(None) => {}
                Err(error) => self.result.failed(error),
            }
        }
        Ok(())
    }
}

/// Sidecars share the transcript extension; only `<id>.jsonl` is a session.
fn is_transcript(path: &Path) -> bool {
    if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
        return false;
    }
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    !name.contains(".checkpoints.") && !name.contains(".prompts.")
}

fn add_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
    mode: SessionHistoryReadMode,
) -> Result<Option<SessionHistoryRecord>, String> {
    let read = || match mode {
        SessionHistoryReadMode::Identities => identity_record(kind, path, fallback_workdir),
        SessionHistoryReadMode::Summaries => summary_record(kind, path, fallback_workdir),
    };
    cached_history_record_facts(path, mode, None, read).map(|facts| facts.map(|facts| facts.record))
}

fn identity_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<HistoryRecordFacts>, String> {
    let Some((id, workdir)) = read_identity(path)? else {
        return Ok(None);
    };
    let workdir = workdir
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    Ok(Some(HistoryRecordFacts {
        record: SessionHistoryRecord::identity(kind, id, &workdir, path.to_path_buf()),
        native_origin_workdir: (!workdir.as_os_str().is_empty()).then_some(workdir),
    }))
}

fn read_identity(path: &Path) -> Result<Option<(String, Option<PathBuf>)>, String> {
    let mut identity = None;
    let header = read_session_header(
        path,
        &["type", "version", "id", "cwd", "sessionId"],
        |fields: &Map<String, Value>, _complete| {
            identity = header_identity(fields);
            identity.is_some()
        },
    )?;
    if header.is_none() {
        return Ok(None);
    }
    Ok(identity)
}

fn header_identity(fields: &Map<String, Value>) -> Option<(String, Option<PathBuf>)> {
    if fields.get("type").and_then(Value::as_str) != Some("session")
        || fields.get("version").and_then(Value::as_u64) != Some(SESSION_VERSION)
    {
        return None;
    }
    let id = field(fields, &["id"])?;
    let workdir = field(fields, &["cwd"])
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    Some((id, workdir))
}

fn summary_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<HistoryRecordFacts>, String> {
    let Some((id, header_workdir)) = read_identity(path)? else {
        return Ok(None);
    };
    let meta = read_meta(path);
    if meta.entrypoint.as_deref() == Some("print") {
        return Ok(None);
    }
    let mut summary = Summary::new(id, meta);
    read_json_lines_limited(
        path,
        SUMMARY_BYTES,
        "读取 Command Code 会话摘要",
        |_, value| summary.observe(&value),
    )?;
    if !summary.saw_header {
        return Ok(None);
    }
    let native_origin_workdir = summary
        .workdir()
        .or(header_workdir)
        .or_else(|| fallback_workdir.map(Path::to_path_buf));
    let workdir = native_origin_workdir
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = first_non_empty([
        summary.title.and_then(|value| clean_text(value, 100)),
        summary.preview.as_ref().and_then(|value| clean_text(value, 100)),
        Path::new(&workdir)
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .and_then(|value| clean_text(value, 100)),
    ]);
    let created_at = summary.created_at.take();
    let summary = CliSessionSummary {
        id: summary.id.clone(),
        title,
        preview: summary.preview,
        model: summary.model.clone(),
        models: summary.model.into_iter().collect(),
        cli_kind: kind,
        created_at: created_at.clone(),
        updated_at: summary
            .updated_at
            .or_else(|| file_modified_at(path))
            .or_else(|| created_at.clone()),
        workdir: workdir.clone(),
        cli_version: None,
        archived: false,
        can_resume: true,
        metadata_source: METADATA_SOURCE.into(),
    };
    let native_origin_workdir =
        (!workdir.is_empty()).then(|| PathBuf::from(&workdir));
    let record = SessionHistoryRecord {
        record_key: summary.id.clone(),
        summary,
        locator: path.to_path_buf(),
        role: AgentSessionRole::Main,
        parent_native_id: None,
        resume_reason: None,
        content_unavailable_reason: None,
    };
    Ok(Some(HistoryRecordFacts {
        record,
        native_origin_workdir,
    }))
}

struct Summary {
    id: String,
    workdir: Option<PathBuf>,
    title: Option<String>,
    preview: Option<String>,
    model: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    saw_header: bool,
}

impl Summary {
    fn new(id: String, meta: Meta) -> Self {
        Self {
            id,
            workdir: None,
            title: meta.title,
            preview: None,
            model: meta.model,
            created_at: None,
            updated_at: None,
            saw_header: false,
        }
    }

    fn workdir(&self) -> Option<PathBuf> {
        self.workdir.clone()
    }

    fn observe(&mut self, value: &Value) {
        match value.get("type").and_then(Value::as_str) {
            Some("session") => {
                let Some(fields) = value.as_object() else {
                    return;
                };
                if header_identity(fields).is_none() {
                    return;
                }
                self.saw_header = true;
                self.workdir = field(fields, &["cwd"])
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute());
                self.created_at = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|value| normalize_timestamp(Some(value)));
            }
            Some("message") => {
                let Some(message) = value.get("message") else {
                    return;
                };
                if message
                    .pointer("/meta/isMeta")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    return;
                }
                if let Some(updated_at) = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|value| normalize_timestamp(Some(value)))
                {
                    self.updated_at = Some(updated_at);
                }
                if let Some(model) = value
                    .get("model")
                    .or_else(|| message.get("model"))
                    .and_then(Value::as_str)
                {
                    self.model = Some(model.to_string());
                }
                if message.get("role").and_then(Value::as_str) == Some("user") {
                    let text = message
                        .get("content")
                        .and_then(user_text)
                        .and_then(|value| clean_text(value, 240));
                    if self.preview.is_none() {
                        self.preview = text;
                    }
                }
            }
            Some("session_info") => {
                if let Some(name) = value
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    self.title = Some(name.to_string());
                }
            }
            Some("model_change") => {
                if let Some(model) = value
                    .get("model")
                    .or_else(|| value.get("to"))
                    .and_then(Value::as_str)
                {
                    self.model = Some(model.to_string());
                }
            }
            _ => {}
        }
    }
}

fn user_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return (!text.trim().is_empty()).then(|| text.to_owned());
    }
    let parts = content.as_array()?;
    let text = parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

#[derive(Default)]
struct Meta {
    title: Option<String>,
    model: Option<String>,
    entrypoint: Option<String>,
}

fn read_meta(path: &Path) -> Meta {
    let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
        return Meta::default();
    };
    let meta_path = path.with_file_name(format!("{name}.meta.json"));
    if !meta_path.is_file() {
        return Meta::default();
    }
    let Ok(text) =
        read_session_metadata_text_file_limited(&meta_path, META_BYTES, "读取 Command Code 会话元数据")
    else {
        return Meta::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Meta::default();
    };
    Meta {
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        entrypoint: value
            .get("entrypoint")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn file_modified_at(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let millis = i64::try_from(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()?;
    crate::services::cli_sessions::timestamp_from_value(Some(millis), true)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Command Code 会话文件不存在".into());
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

fn field(fields: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        fields
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}
