use crate::{
    models::{AgentCliKind, CliSessionMessageRole, CliSessionSummary},
    services::cli_sessions::{
        clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited,
    },
};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

use super::super::contracts::{
    SessionIndexFormat, SessionIndexMessage, SessionIndexMutation, SessionIndexSource,
    SessionMetadataLookupError, SessionMetadataLookupRequest, SessionMetadataLookupResult,
};

const INDEX_PARSER_VERSION: u32 = 2;
mod decoder;

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
        decode: decoder::decode,
    }
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
