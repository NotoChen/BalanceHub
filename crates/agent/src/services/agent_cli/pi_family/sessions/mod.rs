//! Native Pi-family session history.
//!
//! Pi, Prime Agent, omp and OmO write the same append-only JSONL entry model:
//! a `session` header followed by a tree of entries linked by `id`/`parentId`.
//! Only the agent folder and the session root differ, so scanning and summary
//! parsing live here and each Agent supplies a small root specification.

mod decoder;

use super::super::contracts::{
    EnvironmentPatch, SessionHistoryReadMode, SessionHistoryRecord, SessionHistoryScan,
    SessionHistorySource, SessionReadBudget, SessionTranscriptFormat, SessionTranscriptMutation,
    SessionTranscriptSource,
};
use crate::models::{AgentCliKind, AgentSessionRole, CliSessionSummary};
use crate::services::cli_paths::configured_path;
use crate::services::cli_sessions::{
    clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited, timestamp_from_value,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const SUMMARY_BYTES: usize = 256 * 1024;
const INDEX_PARSER_VERSION: u32 = 1;
const MAX_DIRECTORY_DEPTH: usize = 8;
const MAX_SESSION_FILES: usize = 100_000;

/// One family member's session root policy.
#[derive(Clone, Copy)]
pub(crate) struct SessionRootSpec {
    /// Environment variables that override the session root, in priority order.
    pub env_keys: &'static [&'static str],
    /// Whether `settings.json`'s `sessionDir` is honoured (Pi only).
    pub settings_session_dir: bool,
    /// Default session directory below the Agent configuration root.
    pub session_subdir: &'static str,
    /// Agent directory variable restored when resuming a history entry.
    pub launch_env_key: &'static str,
    /// Agent directory below the configuration root named by `launch_env_key`.
    pub launch_env_subdir: &'static str,
    /// Diagnostic source name stored on each session summary.
    pub metadata_source: &'static str,
    /// Whether the Agent's temporary launch adapter can address one session.
    pub resumable: bool,
}

pub(crate) fn source(
    config_root: PathBuf,
    spec: &SessionRootSpec,
) -> Result<SessionHistorySource, String> {
    let (launch_environment, resume_reason) = match launch_environment(&config_root, spec) {
        Ok(environment) => (environment, None),
        Err(reason) => (EnvironmentPatch::default(), Some(reason.to_owned())),
    };
    Ok(SessionHistorySource {
        config_root,
        launch_environment,
        resume_reason,
    })
}

pub(crate) fn launch_environment(
    root: &Path,
    spec: &SessionRootSpec,
) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    let launch_root = if spec.launch_env_subdir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(spec.launch_env_subdir)
    };
    environment.set_path(spec.launch_env_key, &launch_root);
    Ok(environment)
}

pub(crate) fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
    spec: &SessionRootSpec,
) -> Result<SessionHistoryScan, String> {
    let (session_root, diagnostic) = session_root(root, spec);
    let mut result = SessionHistoryScan::new(&[]);
    if let Some(diagnostic) = diagnostic {
        result.failed(diagnostic);
    }
    let mut files = Vec::new();
    if let Err(error) = collect_files(&session_root, budget, &mut result.diagnostics, &mut files) {
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
                summary_record(kind, &path, fallback_workdir, spec)
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

pub(crate) fn transcript(
    record: &SessionHistoryRecord,
) -> Result<Vec<SessionTranscriptSource>, String> {
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

/// Decode one Pi-family event line. Callers that read the same entries from a
/// different container (OpenClaw stores them in SQLite) reuse this parser.
pub(crate) fn decode_entry(
    path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    decoder::decode(path, sequence, line, state)
}

/// The visible text of a Pi message. Used to summarize records whose message
/// entries live outside the JSONL file.
pub(crate) fn message_text(message: &Value) -> Option<String> {
    decoder::message_text(message)
}

fn session_root(root: &Path, spec: &SessionRootSpec) -> (PathBuf, Option<String>) {
    for key in spec.env_keys {
        let Some(value) = configured_path(key) else {
            continue;
        };
        if value.is_absolute() {
            return (value, None);
        }
        return (
            root.join(spec.session_subdir),
            Some(format!("{key} 不是绝对路径，已按默认会话目录读取")),
        );
    }
    if spec.settings_session_dir {
        if let Some(value) =
            super::read_optional_json(&super::settings_path(root)).and_then(|value| {
                value
                    .get("sessionDir")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
        {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return (path, None);
            }
            return (
                root.join(spec.session_subdir),
                Some("sessionDir 是相对路径，未用于会话扫描".into()),
            );
        }
    }
    (root.join(spec.session_subdir), None)
}

/// The directory a temporary launch should write new sessions to. Resolving it
/// with the scan rules keeps `--session-dir` and the session list on the same
/// root; relative overrides fall back exactly as they do when scanning instead
/// of being re-based on the project workdir.
pub(crate) fn session_directory(root: &Path, spec: &SessionRootSpec) -> PathBuf {
    session_root(root, spec).0
}

fn collect_files(
    root: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("会话目录不是普通目录".into());
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        budget.check()?;
        if depth > MAX_DIRECTORY_DEPTH {
            diagnostics.push("会话目录层级过深，已跳过部分记录".into());
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(format!("读取会话目录失败：{error}"));
                continue;
            }
        };
        for entry in entries {
            budget.check()?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    diagnostics.push(format!("读取会话目录项失败：{error}"));
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    diagnostics.push(format!("读取会话目录项类型失败：{error}"));
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
                    .is_some_and(|extension| extension == "jsonl")
            {
                if files.len() >= MAX_SESSION_FILES {
                    diagnostics.push("会话文件数量超过上限，已跳过部分记录".into());
                    return Ok(());
                }
                files.push(entry.path());
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
    let Some(header) = read_session_header(path) else {
        return Ok(None);
    };
    let id = header
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or("Pi 会话记录没有有效的原生会话 ID")?;
    let workdir = header_workdir(&header, fallback_workdir);
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
    spec: &SessionRootSpec,
) -> Result<Option<SessionHistoryRecord>, String> {
    let mut parsed = FileSummary::default();
    let truncated =
        read_json_lines_limited(path, SUMMARY_BYTES, "读取 Pi 会话摘要", |_, value| {
            parsed.observe(&value)
        })?;
    if !parsed.seen_record {
        return if fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0) {
            Ok(None)
        } else {
            Err("Pi 会话记录没有可解析的原生事件".into())
        };
    }
    let Some(id) = parsed
        .id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .or_else(|| id_from_filename(path))
    else {
        return Ok(None);
    };
    let workdir = parsed
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    let first_user_text = parsed.first_user_text.clone();
    let title = first_non_empty([
        parsed.name.and_then(|value| clean_text(value, 100)),
        parsed
            .compaction_summary
            .and_then(|value| clean_text(value, 100)),
        first_user_text
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = first_user_text.and_then(|value| clean_text(value, 240));
    let file_timestamp = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|time| time.to_rfc3339());
    let role = if parsed.rlm_depth > 0 {
        AgentSessionRole::Subagent
    } else {
        AgentSessionRole::Main
    };
    Ok(Some(SessionHistoryRecord {
        summary: CliSessionSummary {
            id: id.clone(),
            title,
            preview,
            model: parsed.last_model,
            models: parsed.models.into_iter().collect(),
            cli_kind: kind,
            created_at: normalize_timestamp(parsed.created_at.as_deref())
                .or_else(|| file_timestamp.clone()),
            updated_at: normalize_timestamp(parsed.updated_at.as_deref()).or(file_timestamp),
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: spec.resumable && (parsed.has_messages || truncated),
            metadata_source: spec.metadata_source.into(),
        },
        locator: path.to_path_buf(),
        record_key: id.clone(),
        role,
        parent_native_id: parsed.parent_session.and_then(|path| {
            Path::new(&path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        }),
        resume_reason: (!spec.resumable).then(|| "该 Agent 暂未接入临时启动续聊".into()),
        content_unavailable_reason: None,
    }))
}

fn read_session_header(path: &Path) -> Option<serde_json::Map<String, Value>> {
    crate::services::cli_sessions::read_session_header(
        path,
        &[
            "type",
            "id",
            "cwd",
            "timestamp",
            "parentSession",
            "rlmDepth",
        ],
        |fields, _| {
            fields.get("type").and_then(Value::as_str) == Some("session")
                && fields
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.trim().is_empty())
        },
    )
    .ok()
    .flatten()
}

fn header_workdir(header: &serde_json::Map<String, Value>, fallback: Option<&Path>) -> PathBuf {
    header
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

fn id_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.trim();
    if stem.is_empty() {
        return None;
    }
    // Pi prefixes the UUID with an ISO-like timestamp; omp and OmO use the
    // session ID first. Only strip a purely numeric prefix.
    let candidate = stem
        .split_once('_')
        .filter(|(prefix, _)| prefix.chars().all(|value| value.is_ascii_digit()))
        .map(|(_, suffix)| suffix)
        .unwrap_or(stem);
    (!candidate.is_empty()).then(|| candidate.to_owned())
}

#[derive(Default)]
struct FileSummary {
    seen_record: bool,
    id: Option<String>,
    cwd: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    name: Option<String>,
    compaction_summary: Option<String>,
    first_user_text: Option<String>,
    last_model: Option<String>,
    models: BTreeSet<String>,
    rlm_depth: i64,
    parent_session: Option<String>,
    has_messages: bool,
}

impl FileSummary {
    fn observe(&mut self, value: &Value) {
        self.seen_record = true;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) {
            self.observe_time(timestamp);
        }
        match kind {
            "session" => {
                self.id = string_field(value, "id").or_else(|| self.id.take());
                self.cwd = string_field(value, "cwd").or_else(|| self.cwd.take());
                self.rlm_depth = value
                    .get("rlmDepth")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                self.parent_session =
                    string_field(value, "parentSession").or_else(|| self.parent_session.take());
            }
            "session_info" => {
                if let Some(name) = string_field(value, "name") {
                    self.name = Some(name);
                }
            }
            "model_change" => {
                if let Some(model) =
                    string_field(value, "modelId").or_else(|| string_field(value, "model"))
                {
                    self.record_model(model);
                }
            }
            "compaction" | "branch_summary" => {
                if let Some(summary) = string_field(value, "summary") {
                    self.compaction_summary = Some(summary);
                }
            }
            "message" => {
                let Some(message) = value.get("message") else {
                    return;
                };
                self.has_messages = true;
                if let Some(timestamp) = message.get("timestamp").and_then(Value::as_i64) {
                    if let Some(timestamp) = timestamp_from_value(Some(timestamp), true) {
                        self.observe_time(&timestamp);
                    }
                }
                match message.get("role").and_then(Value::as_str) {
                    Some("user") if self.first_user_text.is_none() => {
                        self.first_user_text = content_text(message.get("content"))
                            .filter(|text| !text.trim().is_empty());
                    }
                    Some("assistant") => {
                        if let Some(model) = string_field(message, "model") {
                            self.record_model(model);
                        }
                    }
                    _ => {}
                }
            }
            "usage" => {
                if let Some(model) = string_field(value, "model") {
                    self.record_model(model);
                }
            }
            _ => {}
        }
    }

    fn observe_time(&mut self, value: &str) {
        if value.trim().is_empty() {
            return;
        }
        if self.created_at.is_none() {
            self.created_at = Some(value.to_owned());
        }
        self.updated_at = Some(value.to_owned());
    }

    fn record_model(&mut self, model: String) {
        let model = model.trim().to_owned();
        if model.is_empty() {
            return;
        }
        self.models.insert(model.clone());
        self.last_model = Some(model);
    }
}

fn string_field(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn content_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    let blocks = value.as_array()?;
    let text = blocks
        .iter()
        .filter_map(|block| {
            block
                .as_str()
                .map(str::to_owned)
                .or_else(|| block.get("text").and_then(Value::as_str).map(str::to_owned))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
