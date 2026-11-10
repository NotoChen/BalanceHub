//! PenguinHarness trace decoder.
//!
//! Trace shards are envelope records: `{timestamp, type, payload, origin?}`.
//! Complete `model_msg` payloads are the model-visible conversation; streaming
//! `partial_*` and harness state (`event_msg`, `session_meta`) are not rendered
//! as conversation rows.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolKind, SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, normalize_timestamp, presentation, usage},
    },
};
use serde_json::Value;
use std::path::Path;

pub(super) fn decode(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let timestamp = value.get("timestamp").and_then(record_time);
    let payload = value.get("payload").unwrap_or(&Value::Null);
    match value.get("type").and_then(Value::as_str) {
        Some("session_meta") => {
            if let Some(model) = payload.get("model_id").and_then(Value::as_str) {
                set_model(state, model);
            }
            Vec::new()
        }
        Some("model_msg") => decode_model_message(payload, sequence, timestamp, state),
        Some("event_msg") => decode_event(payload, sequence, timestamp),
        Some(kind) if kind.starts_with("partial_") => Vec::new(),
        _ => Vec::new(),
    }
}

fn decode_model_message(
    payload: &Value,
    sequence: u64,
    timestamp: Option<String>,
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or_default();
    let id = payload
        .get("message_id")
        .or_else(|| payload.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("penguin-{sequence}"));
    let model = payload
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| state.get("model").and_then(Value::as_str).map(str::to_owned));
    let role = match payload.get("role").and_then(Value::as_str) {
        Some("user") => CliSessionMessageRole::User,
        Some("tool") => CliSessionMessageRole::Tool,
        // The system prompt is not a conversation row.
        Some("system") => return Vec::new(),
        _ => CliSessionMessageRole::Assistant,
    };
    let mut metadata = SessionMessageMetadata {
        timestamp,
        model,
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let mut result = Vec::new();
    match kind {
        "text" => {
            let Some(text) = payload.get("text").and_then(Value::as_str) else {
                return result;
            };
            if text.trim().is_empty() {
                return result;
            }
            let injected = payload.get("sender").and_then(Value::as_str) == Some("harness");
            metadata.injected = injected;
            push(
                &mut result,
                format!("penguin-{id}"),
                role,
                text.to_owned(),
                SessionMessageMetadata {
                    kind: SessionMessageKind::Text,
                    ..metadata
                },
            );
        }
        "thinking" => {
            if let Some(text) = payload
                .get("thinking")
                .or_else(|| payload.get("text"))
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
            {
                push(
                    &mut result,
                    format!("penguin-{id}:thinking"),
                    CliSessionMessageRole::Assistant,
                    text.to_owned(),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Thinking,
                        ..metadata
                    },
                );
            }
        }
        "tool_call" => {
            let name = payload.get("name").and_then(Value::as_str).unwrap_or("工具");
            let call_id = payload
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(format!("penguin-{id}-call")));
            let arguments = parse_arguments(payload.get("arguments"));
            let mut call = presentation::tool_call(name, &arguments, call_id);
            call.timestamp = metadata.timestamp.clone();
            call.model = metadata.model.clone();
            call.group_id = metadata.group_id.clone();
            push(
                &mut result,
                format!("penguin-{id}:call"),
                CliSessionMessageRole::Tool,
                json_text(&arguments),
                call,
            );
        }
        "tool_call_output" => {
            let call_id = payload
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(format!("penguin-{id}-call")));
            let output = payload.get("output").unwrap_or(&Value::Null);
            let mut result_metadata = presentation::tool_result(output, call_id.clone());
            result_metadata.timestamp = metadata.timestamp.clone();
            result_metadata.model = metadata.model.clone();
            result_metadata.group_id = metadata.group_id.clone();
            result_metadata.tool_kind = Some(SessionToolKind::Other);
            push(
                &mut result,
                format!("penguin-{id}:result"),
                CliSessionMessageRole::Tool,
                json_text(output),
                result_metadata,
            );
            if let Some(images) = payload.get("images").and_then(Value::as_array) {
                for (index, image) in images.iter().enumerate() {
                    let Some(reference) = image_reference(image) else {
                        continue;
                    };
                    push(
                        &mut result,
                        format!("penguin-{id}:result:image:{index:04}"),
                        CliSessionMessageRole::Tool,
                        reference,
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Image,
                            call_id: call_id.clone(),
                            status: Some(SessionToolStatus::Succeeded),
                            tool_kind: Some(SessionToolKind::Other),
                            ..metadata.clone()
                        },
                    );
                }
            }
        }
        "image_url" => {
            if let Some(url) = payload
                .pointer("/image_url/url")
                .or_else(|| payload.pointer("/imageUrl/url"))
                .or_else(|| payload.get("url"))
                .and_then(Value::as_str)
            {
                push(
                    &mut result,
                    format!("penguin-{id}:image"),
                    role,
                    url.to_owned(),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Image,
                        ..metadata
                    },
                );
            }
        }
        "inline_data" => {
            if let Some(value) = image_reference(payload) {
                push(
                    &mut result,
                    format!("penguin-{id}:image"),
                    role,
                    value,
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Image,
                        ..metadata
                    },
                );
            }
        }
        _ => {}
    }
    result
}

fn decode_event(
    payload: &Value,
    sequence: u64,
    timestamp: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    match payload.get("type").and_then(Value::as_str) {
        Some("token_usage") => {
            let request = payload.get("request").unwrap_or(&Value::Null);
            let output = usage::number(request, "output");
            let total = usage::number(request, "total");
            let cache_read = usage::number(request, "cache_read");
            let cache_write = usage::number(request, "cache_write");
            let input = total
                .and_then(|total| output.and_then(|output| total.checked_sub(output)))
                .filter(|input| *input > 0);
            vec![usage::record(
                format!("penguin-usage-{sequence}"),
                None,
                SessionTokenCounts {
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cache_read,
                    cache_write_tokens: cache_write,
                    reasoning_tokens: None,
                },
                false,
                true,
                None,
            )]
        }
        Some("compaction") | Some("context_compaction") => {
            let text = payload
                .get("summary")
                .or_else(|| payload.get("text"))
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty());
            text.map(|text| {
                vec![SessionTranscriptMutation::Put {
                    message: SessionTranscriptMessage {
                        id: format!("penguin-compaction-{sequence}"),
                        role: CliSessionMessageRole::Assistant,
                        content: text.to_owned(),
                        metadata: SessionMessageMetadata {
                            kind: SessionMessageKind::Event,
                            timestamp,
                            ..Default::default()
                        },
                    },
                    priority: 1,
                }]
            })
            .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn push(
    result: &mut Vec<SessionTranscriptMutation>,
    id: String,
    role: CliSessionMessageRole,
    content: String,
    metadata: SessionMessageMetadata,
) {
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

fn parse_arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

fn image_reference(value: &Value) -> Option<String> {
    if let Some(url) = value.get("url").and_then(Value::as_str) {
        return Some(url.to_owned());
    }
    value
        .get("mime_type")
        .or_else(|| value.get("mimeType"))
        .and_then(Value::as_str)
        .map(|mime| format!("data:{mime};base64,[inline image]"))
}

fn record_time(value: &Value) -> Option<String> {
    if let Some(number) = value.as_i64() {
        return crate::services::cli_sessions::timestamp_from_value(Some(number), false);
    }
    value
        .as_str()
        .and_then(|value| normalize_timestamp(Some(value)))
}

fn set_model(state: &mut Value, model: &str) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    if let Some(object) = state.as_object_mut() {
        object.insert("model".into(), Value::String(model.to_owned()));
    }
}
