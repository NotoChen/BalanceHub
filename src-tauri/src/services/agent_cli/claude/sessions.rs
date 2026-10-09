use crate::{
    models::{AgentCliKind, CliSessionDetail, CliSessionMessageRole, CliSessionSummary},
    services::cli_sessions::{
        clean_text, compact_json, first_non_empty, normalize_timestamp, read_json_lines_limited,
        scan_json_lines_matching, SessionContentSearchCollector, SessionMessageCollector,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::Path,
};

use super::super::contracts::{
    SessionContentSearchRequest, SessionContentSearchResult, SessionIndexFormat,
    SessionIndexMessage, SessionIndexMutation, SessionIndexSource, SessionMetadataLookupError,
    SessionMetadataLookupRequest, SessionMetadataLookupResult, SessionReadLimits,
};

const INDEX_PARSER_VERSION: u32 = 1;

mod history;
mod metadata;
mod projects;
pub(super) use history::HISTORY;

pub(super) fn metadata_lookup(
    request: SessionMetadataLookupRequest<'_>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    request.budget.check(0)?;
    if request.workdir.is_none() {
        return Ok(SessionMetadataLookupResult::NotReady);
    }
    let config_dir = super::config::config_dir()
        .ok_or_else(|| SessionMetadataLookupError::Io("无法定位用户目录".to_string()))?;
    metadata::lookup(&config_dir, request)
}

fn index_source(path: &Path) -> SessionIndexSource {
    SessionIndexSource {
        path: path.to_path_buf(),
        parser_version: INDEX_PARSER_VERSION,
        format: SessionIndexFormat::JsonLines,
        decode: index_record,
    }
}

fn index_record(
    path: &Path,
    sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionIndexMutation> {
    if !(line
        .windows(4)
        .any(|part| part.eq_ignore_ascii_case(b"user"))
        || line
            .windows(9)
            .any(|part| part.eq_ignore_ascii_case(b"assistant")))
    {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    if (!history::is_subagent_path(path)
        && value
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false))
        || value
            .get("isMeta")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Vec::new();
    }
    let role = match value.get("type").and_then(Value::as_str) {
        Some("user") => CliSessionMessageRole::User,
        Some("assistant") => CliSessionMessageRole::Assistant,
        _ => return Vec::new(),
    };
    let Some(content) = value
        .get("message")
        .and_then(|message| message.get("content"))
    else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    let mut add = |id: String, text: &str| {
        let text = text.trim();
        if !text.is_empty() && (role != CliSessionMessageRole::User || visible_user_text(text)) {
            messages.push(SessionIndexMutation::Put {
                message: SessionIndexMessage {
                    id,
                    role,
                    content: text.to_owned(),
                },
                priority: 1,
            });
        }
    };
    if let Some(text) = content.as_str() {
        add(format!("claude-{sequence}"), text);
    }
    if let Some(parts) = content.as_array() {
        for (part_index, part) in parts.iter().enumerate() {
            if matches!(
                part.get("type").and_then(Value::as_str),
                Some("text") | None
            ) {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    add(format!("claude-{sequence}-{part_index}"), text);
                }
            }
        }
    }
    messages
}

fn search_transcript(
    path: &Path,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let mut collector = SessionContentSearchCollector::new(request);
    scan_json_lines_matching(
        path,
        "检索 Claude Code 会话正文",
        request,
        is_current,
        |_line_index, value| {
            if (!history::is_subagent_path(path)
                && value
                    .get("isSidechain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false))
                || value
                    .get("isMeta")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                return false;
            }
            let role = match value.get("type").and_then(Value::as_str) {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                _ => return false,
            };
            let Some(content) = value
                .get("message")
                .and_then(|message| message.get("content"))
            else {
                return false;
            };
            if let Some(text) = content.as_str() {
                if role != CliSessionMessageRole::User || visible_user_text(text) {
                    collector.observe(text);
                }
                return collector.complete();
            }
            if let Some(parts) = content.as_array() {
                for part in parts {
                    let text = match part.get("type").and_then(Value::as_str) {
                        Some("text") | None => {
                            let Some(text) = part.get("text").and_then(Value::as_str) else {
                                continue;
                            };
                            if role == CliSessionMessageRole::User && !visible_user_text(text) {
                                continue;
                            }
                            text.to_string()
                        }
                        _ => continue,
                    };
                    collector.observe(&text);
                    if collector.complete() {
                        break;
                    }
                }
            }
            collector.complete()
        },
    )?;
    Ok(collector.finish())
}

fn parse_transcript_messages(
    path: &Path,
    limits: SessionReadLimits,
) -> Result<(Vec<crate::models::CliSessionMessage>, bool, usize), String> {
    let mut collector = SessionMessageCollector::new(limits);
    let mut tool_names = HashMap::<String, String>::new();
    let source_truncated = read_json_lines_limited(
        path,
        limits.max_file_bytes,
        "读取 Claude Code 会话正文",
        |line_index, value| {
            if (!history::is_subagent_path(path)
                && value
                    .get("isSidechain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false))
                || value
                    .get("isMeta")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                return;
            }
            let record_type = value.get("type").and_then(Value::as_str);
            let role = match record_type {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                _ => return,
            };
            let timestamp = normalize_timestamp(value.get("timestamp").and_then(Value::as_str));
            let model = value
                .get("message")
                .and_then(|message| message.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|model| model != "<synthetic>");
            let Some(content) = value
                .get("message")
                .and_then(|message| message.get("content"))
            else {
                return;
            };
            if let Some(text) = content.as_str() {
                if role != CliSessionMessageRole::User || visible_user_text(text) {
                    collector.push(
                        format!("claude-{line_index}"),
                        role,
                        text,
                        timestamp,
                        model,
                        None,
                    );
                }
                return;
            }
            let Some(parts) = content.as_array() else {
                return;
            };
            for (part_index, part) in parts.iter().enumerate() {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") | None => {
                        let Some(text) = part.get("text").and_then(Value::as_str) else {
                            continue;
                        };
                        if role == CliSessionMessageRole::User && !visible_user_text(text) {
                            continue;
                        }
                        collector.push(
                            format!("claude-{line_index}-{part_index}"),
                            role,
                            text,
                            timestamp.clone(),
                            model.clone(),
                            None,
                        );
                    }
                    Some("tool_use") => {
                        let name = part
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("工具")
                            .to_string();
                        if let Some(id) = part.get("id").and_then(Value::as_str) {
                            tool_names.insert(id.to_string(), name.clone());
                        }
                        let input = part
                            .get("input")
                            .map(|value| compact_json(value, limits.max_message_chars))
                            .unwrap_or_default();
                        collector.push(
                            format!("claude-{line_index}-{part_index}"),
                            CliSessionMessageRole::Tool,
                            if input.is_empty() {
                                format!("调用工具 {name}")
                            } else {
                                format!("调用工具 {name}\n{input}")
                            },
                            timestamp.clone(),
                            model.clone(),
                            Some(name),
                        );
                    }
                    Some("tool_result") => {
                        let tool_id = part
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let tool_name = tool_names
                            .get(tool_id)
                            .cloned()
                            .unwrap_or_else(|| "工具结果".to_string());
                        let result = part
                            .get("content")
                            .and_then(content_value_text)
                            .unwrap_or_else(|| compact_json(part, limits.max_message_chars));
                        collector.push(
                            format!("claude-{line_index}-{part_index}"),
                            CliSessionMessageRole::Tool,
                            result,
                            timestamp.clone(),
                            model.clone(),
                            Some(tool_name),
                        );
                    }
                    _ => {}
                }
            }
        },
    )?;
    Ok(collector.finish(source_truncated))
}

struct ParsedClaudeTranscript {
    summary: CliSessionSummary,
    native_origin_workdir: Option<std::path::PathBuf>,
}

fn parse_transcript(
    cli_kind: AgentCliKind,
    path: &Path,
) -> Result<Option<ParsedClaudeTranscript>, String> {
    let mut summary = TranscriptSummary::default();
    let mut valid_record = false;
    let mut observe = |value: &Value| {
        valid_record = true;
        if history::is_subagent_path(path)
            || !value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            summary.observe(value);
        }
    };
    // Summary work stays bounded independently of transcript length. Body
    // search has its own durable checkpoint and does not gate resume eligibility.
    let source_truncated = read_json_lines_limited(
        path,
        256 * 1024,
        "读取 Claude 会话摘要",
        |_, value| observe(&value),
    )?;
    if summary.session_id.is_none() && source_truncated && !valid_record {
        if let Some(header) = crate::services::cli_sessions::read_session_header(
            path,
            &[
                "sessionId",
                "session_id",
                "cwd",
                "workdir",
                "version",
                "timestamp",
                "isSidechain",
            ],
            |fields, _| {
                transcript_identity(&Value::Object(fields.clone()))
                    .0
                    .is_some()
            },
        )? {
            if history::is_subagent_path(path)
                || header.get("isSidechain").and_then(Value::as_bool) != Some(true)
            {
                summary.observe(&Value::Object(header));
                valid_record = true;
            }
        }
    }

    if !valid_record {
        if fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0) {
            return Ok(None);
        }
        return Err("Claude 会话记录没有可解析的原生事件".into());
    }

    let native_origin_workdir = summary.native_origin_workdir.clone();
    let workdir = native_origin_workdir
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let id = summary
        .session_id
        .ok_or("Claude 会话记录没有有效的原生会话 ID")?;
    let first_message = summary.first_user_message.or_else(|| {
        if history::is_subagent_path(path) {
            summary.first_assistant_message
        } else {
            None
        }
    });
    let title = first_non_empty([
        summary
            .latest_ai_title
            .and_then(|value| clean_text(value, 100)),
        first_message
            .clone()
            .and_then(|value| clean_text(value, 100)),
    ]);
    let preview = first_message.and_then(|value| clean_text(value, 240));
    let model = summary.last_model.clone();
    let models = summary.models.into_iter().collect::<Vec<_>>();
    Ok(Some(ParsedClaudeTranscript {
        summary: CliSessionSummary {
            id,
            title,
            preview,
            model,
            models,
            cli_kind,
            created_at: normalize_timestamp(summary.created_at.as_deref()),
            updated_at: normalize_timestamp(summary.updated_at.as_deref()).or_else(|| {
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .map(chrono::DateTime::<chrono::Utc>::from)
                    .map(|time| time.to_rfc3339())
            }),
            workdir,
            cli_version: summary.cli_version.and_then(|value| clean_text(value, 50)),
            archived: false,
            can_resume: true,
            metadata_source: "claudeTranscript".into(),
        },
        native_origin_workdir,
    }))
}

#[derive(Default)]
struct TranscriptSummary {
    session_id: Option<String>,
    native_origin_workdir: Option<std::path::PathBuf>,
    cli_version: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    first_user_message: Option<String>,
    first_assistant_message: Option<String>,
    latest_ai_title: Option<String>,
    last_model: Option<String>,
    models: BTreeSet<String>,
}

impl TranscriptSummary {
    fn observe(&mut self, value: &Value) {
        let (session_id, native_origin_workdir) = transcript_identity(value);
        self.session_id = self
            .session_id
            .take()
            .filter(|value| !value.trim().is_empty())
            .or(session_id);
        if self.native_origin_workdir.is_none() {
            self.native_origin_workdir = native_origin_workdir;
        }
        self.cli_version = self
            .cli_version
            .take()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| string_field(value, &["version", "cliVersion"]));
        let timestamp = string_field(value, &["timestamp", "createdAt"]);
        if self.created_at.is_none() {
            self.created_at = timestamp.clone();
        }
        if timestamp.is_some() {
            self.updated_at = timestamp;
        }

        if value.get("type").and_then(Value::as_str) == Some("ai-title") {
            if let Some(title) = string_field(value, &["aiTitle", "title", "summary", "name"]) {
                if !title.trim().is_empty() {
                    self.latest_ai_title = Some(title);
                }
            }
        }
        if value.get("type").and_then(Value::as_str) == Some("user")
            && !value
                .get("isMeta")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && self.first_user_message.is_none()
        {
            self.first_user_message = message_text(value);
        }
        if value.get("type").and_then(Value::as_str) == Some("assistant") {
            if self.first_assistant_message.is_none()
                && !value
                    .get("isMeta")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                self.first_assistant_message = message_text(value);
            }
            if let Some(model) = value
                .get("message")
                .and_then(|message| message.get("model"))
                .and_then(Value::as_str)
                .filter(|model| !model.trim().is_empty() && *model != "<synthetic>")
            {
                let model = model.trim().to_string();
                self.models.insert(model.clone());
                self.last_model = Some(model);
            }
        }
    }
}

fn transcript_identity(value: &Value) -> (Option<String>, Option<std::path::PathBuf>) {
    (
        ["sessionId", "session_id"].into_iter().find_map(|field| {
            value
                .get(field)?
                .as_str()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        }),
        ["cwd", "workdir"].into_iter().find_map(|field| {
            let path = value.get(field)?.as_str()?;
            (Path::new(path).is_absolute() && !path.chars().any(char::is_control))
                .then(|| std::path::PathBuf::from(path))
        }),
    )
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn message_text(value: &Value) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return user_text_candidate(text);
    }
    let parts = content.as_array()?.iter().filter_map(|part| {
        let object = part.as_object()?;
        match object.get("type").and_then(Value::as_str) {
            Some("text") | None => object.get("text").and_then(Value::as_str),
            _ => None,
        }
    });
    let text = parts.collect::<Vec<_>>().join(" ");
    user_text_candidate(&text)
}

fn content_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
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

fn visible_user_text(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with("<command-name")
        && !value.starts_with("<local-command")
        && !value.starts_with("<command-message")
}

fn user_text_candidate(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with("<command-name")
        || value.starts_with("<local-command")
        || value.starts_with("<command-message")
    {
        return None;
    }
    Some(value.to_string())
}

fn path_key(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut value = path.to_string_lossy().replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        value.make_ascii_lowercase();
    }
    value
}

fn encode_project_path(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
