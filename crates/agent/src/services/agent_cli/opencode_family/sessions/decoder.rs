//! OpenCode-family part decoder.
//!
//! Parts arrive joined with their parent message so role, model and token
//! totals are available without a second query. The part id is the stable
//! identity; a tool part produces a call row and, when it has an outcome, a
//! result row that shares the native call id.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{clean_text, json_text, presentation, timestamp_from_value, usage},
    },
};
use serde_json::Value;
use std::path::Path;

pub(super) fn decode(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(payload) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let part = payload.get("part").cloned().unwrap_or(Value::Null);
    let message = payload.get("message").cloned().unwrap_or(Value::Null);
    let part_id = payload
        .get("part_id")
        .or_else(|| part.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("opencode-part-{sequence}"));
    let message_id = payload
        .get("message_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let model = model_from_message(&message);
    let timestamp = payload
        .get("part_time")
        .or_else(|| payload.get("message_time"))
        .and_then(Value::as_i64)
        .and_then(|value| timestamp_from_value(Some(value), true));
    let base = SessionMessageMetadata {
        timestamp,
        model: model.clone(),
        group_id: message_id.clone(),
        ..Default::default()
    };
    let mut result = Vec::new();
    if role == "assistant" {
        if let Some(record) = usage_from_message(message_id.as_deref(), model.as_deref(), &message)
        {
            result.push(record);
        }
    }
    match part.get("type").and_then(Value::as_str).unwrap_or_default() {
        "text" => {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                push(
                    &mut result,
                    part_id,
                    if role == "user" {
                        CliSessionMessageRole::User
                    } else {
                        CliSessionMessageRole::Assistant
                    },
                    text.to_owned(),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Text,
                        ..base
                    },
                );
            }
        }
        "reasoning" => {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                push(
                    &mut result,
                    part_id,
                    CliSessionMessageRole::Assistant,
                    text.to_owned(),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Thinking,
                        ..base
                    },
                );
            }
        }
        "tool" => {
            let call_id = part
                .get("callID")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| part_id.clone());
            let name = part.get("tool").and_then(Value::as_str).unwrap_or("工具");
            let state = part.get("state").unwrap_or(&Value::Null);
            let status = tool_status(state);
            let input = state.get("input").cloned().unwrap_or(Value::Null);
            let mut metadata = presentation::tool_call(name, &input, Some(call_id.clone()));
            metadata.timestamp = base.timestamp.clone();
            metadata.model = base.model.clone();
            metadata.group_id = base.group_id.clone();
            metadata.status = status.filter(|status| *status == SessionToolStatus::Running);
            if let Some(title) = state
                .get("title")
                .and_then(Value::as_str)
                .and_then(|title| clean_text(title, 180))
            {
                metadata.title = Some(title);
            }
            push(
                &mut result,
                format!("{part_id}:call"),
                CliSessionMessageRole::Tool,
                json_text(&input),
                metadata,
            );
            let output = state.get("output").or_else(|| state.get("error"));
            if let Some(output) = output.filter(|value| !value.is_null()) {
                let mut metadata = presentation::tool_result(output, Some(call_id));
                metadata.timestamp = base.timestamp.clone();
                metadata.model = base.model.clone();
                metadata.group_id = base.group_id.clone();
                metadata.status = status;
                push(
                    &mut result,
                    format!("{part_id}:result"),
                    CliSessionMessageRole::Tool,
                    render_result(output),
                    metadata,
                );
            }
        }
        "file" => {
            let filename = part
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or("附件");
            let mime = part
                .get("mime")
                .or_else(|| part.get("mimeType"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let url = part.get("url").and_then(Value::as_str).unwrap_or_default();
            let image = mime.starts_with("image/");
            let content = if url.starts_with("data:") || url.is_empty() {
                format!(
                    "{} {filename}{}",
                    if image { "![内联图片]" } else { "[附件]" },
                    if mime.is_empty() {
                        String::new()
                    } else {
                        format!(" ({mime})")
                    }
                )
            } else if image {
                format!("![{filename}]({url})")
            } else {
                format!("[{filename}]({url})")
            };
            push(
                &mut result,
                part_id,
                CliSessionMessageRole::Assistant,
                content,
                SessionMessageMetadata {
                    kind: if image {
                        SessionMessageKind::Image
                    } else {
                        SessionMessageKind::Event
                    },
                    ..base
                },
            );
        }
        "patch" => {
            let files = part
                .get("files")
                .and_then(Value::as_array)
                .map(|files| {
                    files
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|files| !files.is_empty());
            let text = files
                .map(|files| format!("[补丁] {files}"))
                .unwrap_or_else(|| "[补丁]".into());
            push(
                &mut result,
                part_id,
                CliSessionMessageRole::Assistant,
                text,
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    ..base
                },
            );
        }
        "agent" | "subtask" => {
            let name = part
                .get("name")
                .or_else(|| part.get("agent"))
                .or_else(|| part.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("子代理");
            push(
                &mut result,
                part_id,
                CliSessionMessageRole::Assistant,
                format!("[子代理] {name}"),
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    ..base
                },
            );
        }
        "compaction" => {
            let text = if part.get("auto").and_then(Value::as_bool) == Some(true) {
                "[自动压缩上下文]"
            } else {
                "[压缩上下文]"
            };
            push(
                &mut result,
                part_id,
                CliSessionMessageRole::Assistant,
                text.to_owned(),
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    injected: true,
                    ..base
                },
            );
        }
        "retry" => {
            if let Some(error) = part
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
            {
                push(
                    &mut result,
                    part_id,
                    CliSessionMessageRole::Assistant,
                    format!("[重试] {error}"),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Event,
                        ..base
                    },
                );
            }
        }
        _ => {}
    }
    result
}

fn model_from_message(message: &Value) -> Option<String> {
    message
        .get("model")
        .and_then(|model| {
            model
                .get("modelID")
                .or_else(|| model.get("id"))
                .and_then(Value::as_str)
        })
        .or_else(|| message.get("modelID").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn usage_from_message(
    message_id: Option<&str>,
    model: Option<&str>,
    message: &Value,
) -> Option<SessionTranscriptMutation> {
    let tokens = message.get("tokens")?;
    let counts = SessionTokenCounts {
        input_tokens: number(tokens, "input"),
        output_tokens: number(tokens, "output"),
        reasoning_tokens: number(tokens, "reasoning"),
        cache_read_tokens: tokens.get("cache").and_then(|cache| number(cache, "read")),
        cache_write_tokens: tokens.get("cache").and_then(|cache| number(cache, "write")),
    };
    if counts == SessionTokenCounts::default() {
        return None;
    }
    Some(usage::record(
        message_id?.to_owned(),
        model,
        counts,
        false,
        false,
        message_id,
    ))
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
}

fn tool_status(state: &Value) -> Option<SessionToolStatus> {
    match state.get("status").and_then(Value::as_str) {
        Some("completed") => Some(SessionToolStatus::Succeeded),
        Some("error") => Some(SessionToolStatus::Failed),
        Some("running") | Some("pending") => Some(SessionToolStatus::Running),
        _ => None,
    }
}

fn render_result(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(_) => value
            .get("output")
            .or_else(|| value.get("error"))
            .map(render_result)
            .unwrap_or_else(|| json_text(value)),
        _ => json_text(value),
    }
}

fn push(
    result: &mut Vec<SessionTranscriptMutation>,
    id: String,
    role: CliSessionMessageRole,
    content: String,
    metadata: SessionMessageMetadata,
) {
    if content.trim().is_empty() {
        return;
    }
    result.push(SessionTranscriptMutation::Put {
        message: SessionTranscriptMessage {
            id,
            role,
            content,
            metadata,
        },
        priority: 1,
    });
}
