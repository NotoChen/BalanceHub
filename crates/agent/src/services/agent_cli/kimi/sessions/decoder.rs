//! Kimi Code wire-journal decoder.
//!
//! `context.append_message` records carry one complete model-visible message;
//! `context.clear` resets the derived history. Streaming loop events are
//! ignored because the complete message records already contain them.

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
    match value.get("type").and_then(Value::as_str) {
        Some("context.append_message") => {
            decode_message(&value, sequence, state)
        }
        Some("context.clear") => vec![SessionTranscriptMutation::Clear],
        Some("context.apply_compaction") => decode_compaction(&value, sequence),
        Some("config.update") => {
            if let Some(model) = value.get("modelAlias").and_then(Value::as_str) {
                set_model(state, model);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn decode_message(value: &Value, sequence: u64, state: &mut Value) -> Vec<SessionTranscriptMutation> {
    let Some(message) = value.get("message") else {
        return Vec::new();
    };
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("kimi-{sequence}"));
    let timestamp = value.get("time").and_then(record_time);
    let model = state.get("model").and_then(Value::as_str).map(str::to_owned);
    let base = SessionMessageMetadata {
        timestamp,
        model,
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
    let mut result = Vec::new();
    if let Some(parts) = message.get("content").and_then(Value::as_array) {
        let mut text = Vec::new();
        for (part_index, part) in parts.iter().enumerate() {
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = part.get("text").and_then(Value::as_str) {
                        text.push(value.to_owned());
                    }
                }
                Some("think") => {
                    if let Some(value) = part.get("think").and_then(Value::as_str) {
                        push_text(
                            &mut result,
                            format!("kimi-{id}:thinking:{part_index:06}"),
                            CliSessionMessageRole::Assistant,
                            value.to_owned(),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Thinking,
                                ..base.clone()
                            },
                            SessionMessageKind::Thinking,
                        );
                    }
                }
                Some("image_url") => {
                    if let Some(value) = part
                        .pointer("/imageUrl/url")
                        .or_else(|| part.pointer("/image_url/url"))
                        .and_then(Value::as_str)
                    {
                        push_text(
                            &mut result,
                            format!("kimi-{id}:image:{part_index:06}"),
                            CliSessionMessageRole::User,
                            value.to_owned(),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Image,
                                ..base.clone()
                            },
                            SessionMessageKind::Image,
                        );
                    }
                }
                _ => {}
            }
        }
        if text.iter().any(|value| !value.trim().is_empty()) {
            let role = match role {
                "user" => CliSessionMessageRole::User,
                "tool" => CliSessionMessageRole::Tool,
                // The system prompt is not a conversation row.
                "system" => return result,
                _ => CliSessionMessageRole::Assistant,
            };
            push_text(
                &mut result,
                format!("kimi-{id}"),
                role,
                text.join("\n"),
                base.clone(),
                SessionMessageKind::Text,
            );
        }
    }
    if role == "tool" {
        if let Some(call_id) = message.get("toolCallId").and_then(Value::as_str) {
            let failed = message
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            for mutation in &mut result {
                if let SessionTranscriptMutation::Put {
                    message: transcript,
                    ..
                } = mutation
                {
                    if transcript.metadata.call_id.is_none() {
                        transcript.metadata.call_id = Some(call_id.to_owned());
                        transcript.metadata.tool_kind = Some(SessionToolKind::Other);
                        transcript.metadata.status = Some(if failed {
                            SessionToolStatus::Failed
                        } else {
                            SessionToolStatus::Succeeded
                        });
                    }
                }
            }
        }
    }
    if let Some(calls) = message.get("toolCalls").and_then(Value::as_array) {
        for (index, call) in calls.iter().enumerate() {
            let name = call.get("name").and_then(Value::as_str).unwrap_or("工具");
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(format!("kimi-{id}-call-{index}")));
            let arguments = parse_arguments(call.get("arguments"));
            let mut metadata = presentation::tool_call(name, &arguments, call_id);
            metadata.timestamp = base.timestamp.clone();
            metadata.model = base.model.clone();
            metadata.group_id = base.group_id.clone();
            push_text(
                &mut result,
                format!("kimi-{id}:call:{index:06}"),
                CliSessionMessageRole::Tool,
                json_text(&arguments),
                metadata,
                SessionMessageKind::ToolCall,
            );
        }
    }
    if let Some(usage_value) = message.get("usage") {
        result.push(usage::record(
            format!("kimi-usage-{id}"),
            None,
            SessionTokenCounts {
                input_tokens: usage_value.get("inputOther").and_then(Value::as_u64),
                output_tokens: usage_value.get("output").and_then(Value::as_u64),
                cache_read_tokens: usage_value.get("inputCacheRead").and_then(Value::as_u64),
                cache_write_tokens: usage_value.get("inputCacheCreation").and_then(Value::as_u64),
                reasoning_tokens: None,
            },
            false,
            false,
            Some(&format!("kimi-{id}")),
        ));
    }
    result
}

fn decode_compaction(value: &Value, sequence: u64) -> Vec<SessionTranscriptMutation> {
    let text = value
        .get("summary")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/contextSummary").and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty());
    let Some(text) = text else {
        return Vec::new();
    };
    vec![SessionTranscriptMutation::Put {
        message: SessionTranscriptMessage {
            id: format!("kimi-compaction-{sequence}"),
            role: CliSessionMessageRole::Assistant,
            content: text.to_owned(),
            metadata: SessionMessageMetadata {
                kind: SessionMessageKind::Event,
                timestamp: value.get("time").and_then(record_time),
                ..Default::default()
            },
        },
        priority: 1,
    }]
}

fn push_text(
    result: &mut Vec<SessionTranscriptMutation>,
    id: String,
    role: CliSessionMessageRole,
    content: String,
    mut metadata: SessionMessageMetadata,
    kind: SessionMessageKind,
) {
    metadata.kind = kind;
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
