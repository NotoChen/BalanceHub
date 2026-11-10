//! Qwen Code native JSONL history.
//!
//! Qwen keeps one append-only ChatRecord stream per session under
//! `projects/<project>/chats/<sessionId>.jsonl`. Records form a `uuid` /
//! `parentUuid` tree. Sub-agent logs live under `subagents/<session>/`.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode,
            SessionHistoryRecord, SessionHistoryScan, SessionHistorySource, SessionReadBudget,
            SessionTranscriptFormat, SessionTranscriptSource,
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
        return Err("Qwen Code 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("QWEN_RUNTIME_DIR", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root =
        config::runtime_base_dir().ok_or_else(|| "无法定位 Qwen Code 原生目录".to_string())?;
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
    let mut candidates = Vec::new();
    if let Err(error) = collect(&root.join("projects"), budget, &mut result.diagnostics, &mut candidates) {
        result.failed(error);
        return Ok(result);
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    for candidate in candidates {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                identity_record(kind, &candidate, fallback_workdir)
            }
            SessionHistoryReadMode::Summaries => {
                summary_record(kind, &candidate, fallback_workdir)
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

struct Candidate {
    path: PathBuf,
    parent_session: Option<String>,
}

fn collect(
    projects: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Candidate>,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(projects) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Qwen 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Qwen 会话目录不是普通目录".into());
    }
    for entry in fs::read_dir(projects).map_err(|error| error.to_string())? {
        budget.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let project = entry.path();
        collect_chats(&project.join("chats"), budget, diagnostics, out)?;
        collect_subagents(&project.join("subagents"), budget, diagnostics, out)?;
    }
    Ok(())
}

fn collect_chats(
    chats: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Candidate>,
) -> Result<(), String> {
    let entries = match fs::read_dir(chats) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            diagnostics.push(format!("读取 Qwen 会话目录失败：{error}"));
            return Ok(());
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
        if entry.file_type().is_ok_and(|kind| kind.is_file())
            && entry.path().extension().is_some_and(|ext| ext == "jsonl")
        {
            if out.len() >= MAX_SESSION_FILES {
                diagnostics.push("Qwen 会话文件数量超过上限，已跳过部分记录".into());
                return Ok(());
            }
            out.push(Candidate {
                path: entry.path(),
                parent_session: None,
            });
        }
    }
    Ok(())
}

fn collect_subagents(
    root: &Path,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Candidate>,
) -> Result<(), String> {
    let directories = match fs::read_dir(root) {
        Ok(directories) => directories,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            diagnostics.push(format!("读取 Qwen 子代理目录失败：{error}"));
            return Ok(());
        }
    };
    for directory in directories {
        budget.check()?;
        let directory = match directory {
            Ok(directory) => directory,
            Err(error) => {
                diagnostics.push(error.to_string());
                continue;
            }
        };
        if !directory.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let parent = directory.file_name().to_string_lossy().to_string();
        collect_jsonl_tree(
            &directory.path(),
            0,
            budget,
            diagnostics,
            out,
            Some(parent),
        )?;
    }
    Ok(())
}

fn collect_jsonl_tree(
    root: &Path,
    depth: usize,
    budget: &SessionReadBudget,
    diagnostics: &mut Vec<String>,
    out: &mut Vec<Candidate>,
    parent: Option<String>,
) -> Result<(), String> {
    if depth > MAX_DIRECTORY_DEPTH {
        diagnostics.push("Qwen 子代理目录层级过深，已跳过部分记录".into());
        return Ok(());
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            diagnostics.push(format!("读取 Qwen 子代理记录失败：{error}"));
            return Ok(());
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
            collect_jsonl_tree(
                &entry.path(),
                depth + 1,
                budget,
                diagnostics,
                out,
                parent.clone(),
            )?;
        } else if file_type.is_file()
            && entry.path().extension().is_some_and(|ext| ext == "jsonl")
        {
            if out.len() >= MAX_SESSION_FILES {
                diagnostics.push("Qwen 会话文件数量超过上限，已跳过部分记录".into());
                return Ok(());
            }
            out.push(Candidate {
                path: entry.path(),
                parent_session: parent.clone(),
            });
        }
    }
    Ok(())
}

fn identity_record(
    kind: AgentCliKind,
    candidate: &Candidate,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(header) = read_header(&candidate.path) else {
        return Ok(None);
    };
    let id = native_session_id(&header)
        .or_else(|| file_session_id(&candidate.path))
        .ok_or("Qwen 会话记录没有有效的原生会话 ID")?;
    let workdir = header_workdir(&header, fallback_workdir);
    Ok(Some(SessionHistoryRecord::identity(
        kind,
        id,
        &workdir,
        candidate.path.clone(),
    )))
}

fn summary_record(
    kind: AgentCliKind,
    candidate: &Candidate,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let mut parsed = FileSummary::default();
    let truncated = read_json_lines_limited(
        &candidate.path,
        SUMMARY_BYTES,
        "读取 Qwen Code 会话摘要",
        |_, value| parsed.observe(&value),
    )?;
    if !parsed.seen_record {
        return if fs::metadata(&candidate.path).is_ok_and(|metadata| metadata.len() == 0) {
            Ok(None)
        } else {
            Err("Qwen 会话记录没有可解析的原生事件".into())
        };
    }
    let Some(id) = parsed
        .session_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .or_else(|| file_session_id(&candidate.path))
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
        parsed
            .title
            .and_then(|value| clean_text(value, 100)),
        first_user_text
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = first_user_text.and_then(|value| clean_text(value, 240));
    let file_timestamp = fs::metadata(&candidate.path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|time| time.to_rfc3339());
    let is_subagent = candidate.parent_session.is_some();
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
            can_resume: !is_subagent && (parsed.has_messages || truncated),
            metadata_source: "qwenTranscript".into(),
        },
        locator: candidate.path.clone(),
        record_key: id,
        role: if is_subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: candidate.parent_session.clone(),
        resume_reason: is_subagent.then(|| "Qwen 子代理会话需由父会话继续".into()),
        content_unavailable_reason: None,
    }))
}

fn read_header(path: &Path) -> Option<serde_json::Map<String, Value>> {
    crate::services::cli_sessions::read_session_header(
        path,
        &["sessionId", "uuid", "cwd", "timestamp", "type", "message"],
        |fields, _| {
            native_session_id(fields).is_some()
                || fields.get("cwd").and_then(Value::as_str).is_some()
        },
    )
    .ok()
    .flatten()
}

fn header_workdir(
    header: &serde_json::Map<String, Value>,
    fallback: Option<&Path>,
) -> PathBuf {
    header
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback.map(Path::to_path_buf))
        .unwrap_or_default()
}

fn native_session_id(value: &serde_json::Map<String, Value>) -> Option<String> {
    value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("uuid")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        })
}

fn file_session_id(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.trim();
    (!stem.is_empty()).then(|| stem.to_owned())
}

#[derive(Default)]
struct FileSummary {
    seen_record: bool,
    session_id: Option<String>,
    cwd: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    title: Option<String>,
    first_user_text: Option<String>,
    last_model: Option<String>,
    models: BTreeSet<String>,
    has_messages: bool,
}

impl FileSummary {
    fn observe(&mut self, value: &Value) {
        self.seen_record = true;
        if let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) {
            self.observe_time(timestamp);
        }
        self.session_id = string_field(value, "sessionId").or_else(|| self.session_id.take());
        self.cwd = string_field(value, "cwd").or_else(|| self.cwd.take());
        let record_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
        let message = value.get("message");
        let role = message
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            .or(match record_type {
                "user" => Some("user"),
                "assistant" => Some("model"),
                "tool_result" => Some("tool"),
                _ => None,
            });
        if self.title.is_none() {
            if let Some(title) = value.pointer("/systemPayload/title").and_then(Value::as_str) {
                self.title = Some(title.to_owned());
            }
        }
        if record_type == "system" {
            return;
        }
        if let Some(message) = message {
            if let Some(model) = string_field(message, "model")
                .or_else(|| string_field(value, "model"))
            {
                self.record_model(model);
            }
        }
        if let Some(model) = string_field(value, "model") {
            self.record_model(model);
        }
        let text = message.and_then(message_text);
        let has_text = text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty());
        if role == Some("user") {
            if self.first_user_text.is_none() {
                self.first_user_text = text.filter(|text| !text.trim().is_empty());
            }
            if has_text {
                self.has_messages = true;
            }
        } else if role == Some("model") && has_text {
            self.has_messages = true;
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

fn message_text(message: &Value) -> Option<String> {
    if let Some(text) = message
        .get("parts")
        .and_then(Value::as_array)
        .and_then(|parts| parts_text(parts))
    {
        return Some(text);
    }
    if let Some(parts) = message.as_array() {
        return parts_text(parts);
    }
    message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn parts_text(parts: &[Value]) -> Option<String> {
    let text = parts
        .iter()
        .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
