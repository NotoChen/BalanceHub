//! CodeBuddy native session history.
//!
//! CodeBuddy is a Claude Code fork with its own JSONL schema: OpenAI
//! Responses-style message and function-call items plus title records. Files
//! live under `projects/<workspace>/<session>.jsonl`, with subagents below
//! `<session>/subagents/agent-*.jsonl`.

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
        read_session_header,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
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
        return Err("CodeBuddy 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("CODEBUDDY_CONFIG_DIR", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_dir().ok_or_else(|| "无法定位 CodeBuddy 配置目录".to_string())?;
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
    if let Err(error) = collect(
        &root.join("projects"),
        budget,
        &mut result.diagnostics,
        &mut candidates,
    ) {
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
    subagent: bool,
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
        Err(error) => return Err(format!("CodeBuddy 会话目录不可读：{error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("CodeBuddy 会话目录不是普通目录".into());
    }
    let mut stack = vec![(projects.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        budget.check()?;
        if depth > MAX_DIRECTORY_DEPTH {
            diagnostics.push("CodeBuddy 会话目录层级过深，已跳过部分记录".into());
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(format!("读取 CodeBuddy 会话目录失败：{error}"));
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
                    .is_some_and(|extension| extension == "jsonl")
            {
                if out.len() >= MAX_SESSION_FILES {
                    diagnostics.push("CodeBuddy 会话文件数量超过上限，已跳过部分记录".into());
                    return Ok(());
                }
                let subagent = entry
                    .path()
                    .components()
                    .any(|component| component.as_os_str() == "subagents");
                out.push(Candidate {
                    parent_session: parent_session_id(&entry.path()),
                    path: entry.path(),
                    subagent,
                });
            }
        }
    }
    Ok(())
}

fn parent_session_id(path: &Path) -> Option<String> {
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        if component.as_os_str() != "subagents" || index == 0 {
            continue;
        }
        return match components[index - 1] {
            Component::Normal(value) => value
                .to_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            _ => None,
        };
    }
    None
}

fn native_session_id(candidate: &Candidate) -> Option<String> {
    candidate
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
}

fn identity_record(
    kind: AgentCliKind,
    candidate: &Candidate,
    fallback_workdir: Option<&Path>,
) -> Result<Option<SessionHistoryRecord>, String> {
    let Some(id) = native_session_id(candidate) else {
        return Ok(None);
    };
    let workdir = read_header(&candidate.path)
        .and_then(|header| header_workdir(&header))
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
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
    let Some(id) = native_session_id(candidate) else {
        return Ok(None);
    };
    let mut parsed = FileSummary::default();
    let _truncated = read_json_lines_limited(
        &candidate.path,
        SUMMARY_BYTES,
        "读取 CodeBuddy 会话摘要",
        |_, value| parsed.observe(&value),
    )?;
    if !parsed.seen_record {
        return if fs::metadata(&candidate.path).is_ok_and(|metadata| metadata.len() == 0) {
            Ok(None)
        } else {
            Err("CodeBuddy 会话记录没有可解析的原生事件".into())
        };
    }
    let workdir = parsed
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    let first_user_text = parsed.first_user_text.clone();
    let title = first_non_empty([
        parsed.title.and_then(|value| clean_text(value, 100)),
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
            can_resume: !candidate.subagent && parsed.has_messages,
            metadata_source: "codebuddyTranscript".into(),
        },
        locator: candidate.path.clone(),
        record_key: if candidate.subagent {
            format!("subagent:{}", candidate.path.to_string_lossy())
        } else {
            id
        },
        role: if candidate.subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: candidate.parent_session.clone(),
        resume_reason: candidate
            .subagent
            .then(|| "CodeBuddy 子代理会话需由父会话继续".into()),
        content_unavailable_reason: None,
    }))
}

fn read_header(path: &Path) -> Option<serde_json::Map<String, Value>> {
    read_session_header(path, &["type", "role", "cwd", "timestamp", "message"], |fields, _| {
        fields
            .get("cwd")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
            || fields.get("type").and_then(Value::as_str) == Some("message")
    })
    .ok()
    .flatten()
}

fn header_workdir(header: &serde_json::Map<String, Value>) -> Option<PathBuf> {
    header
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[derive(Default)]
struct FileSummary {
    seen_record: bool,
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
        if let Some(timestamp) = value
            .get("timestamp")
            .and_then(value_timestamp)
            .or_else(|| value.get("time").and_then(value_timestamp))
        {
            self.observe_time(&timestamp);
        }
        if let Some(cwd) = value
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.cwd = Some(cwd.to_owned());
        }
        let record_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
        match record_type {
            "message" => self.observe_message(value),
            "reasoning" => self.has_messages = true,
            "custom-title" | "customTitle" | "ai-title" | "aiTitle" | "topic" => {
                if let Some(title) = title_text(value) {
                    self.title = Some(title);
                }
            }
            "summary" if self.title.is_none() => {
                self.title = value
                    .get("summary")
                    .and_then(Value::as_str)
                    .and_then(|text| clean_text(text, 100));
            }
            "session-meta" | "session_meta" => {
                if let Some(model) = string_field(value, &["model", "modelId"]) {
                    self.record_model(model);
                }
            }
            _ => {}
        }
    }

    fn observe_message(&mut self, value: &Value) {
        if value
            .pointer("/providerData/skipRun")
            .and_then(Value::as_bool)
            == Some(true)
        {
            return;
        }
        let role = value.get("role").and_then(Value::as_str).unwrap_or_default();
        if let Some(model) = string_field(value, &["model"]).or_else(|| {
            value
                .pointer("/providerData/model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }) {
            self.record_model(model);
        }
        let text = content_text(value.get("content"));
        let has_text = text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty());
        match role {
            "user" => {
                if self.first_user_text.is_none() {
                    self.first_user_text = text.filter(|text| !text.trim().is_empty());
                }
                self.has_messages |= has_text;
            }
            "assistant" => self.has_messages |= has_text,
            _ => {}
        }
    }

    fn observe_time(&mut self, value: &str) {
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

fn value_timestamp(value: &Value) -> Option<String> {
    if let Some(number) = value.as_i64() {
        return crate::services::cli_sessions::timestamp_from_value(Some(number), false);
    }
    value
        .as_str()
        .and_then(|value| normalize_timestamp(Some(value)))
}

fn title_text(value: &Value) -> Option<String> {
    ["title", "customTitle", "custom_title", "aiTitle", "ai_title", "topic", "text"]
        .iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .and_then(|text| clean_text(text, 100))
        })
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
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
