//! Cline message-list decoder.
//!
//! Cline stores Anthropic-shaped messages in `<id>.messages.json`. Tool calls
//! and results are content blocks on user/assistant messages rather than
//! separate rows, so each block becomes its own transcript row.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolKind,
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
    // `Messages` hands the reader one element of the `messages` array.
    // The caller re-serializes it as the line; parse it again here.
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    decode_message(&value, sequence, state)
}

fn decode_message(
    value: &Value,
    sequence: u64,
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let message = value.get("message").unwrap_or(value);
    let id = message
        .get("id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("cline-{sequence}"));
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // The system prompt is not a conversation row.
    if role == "system" {
        return Vec::new();
    }
    let timestamp = message
        .get("ts")
        .or_else(|| message.get("timestamp"))
        .or_else(|| value.get("ts"))
        .and_then(record_time);
    if let Some(model) = message
        .pointer("/modelInfo/modelId")
        .or_else(|| message.pointer("/modelInfo/model"))
        .or_else(|| message.get("model"))
        .and_then(Value::as_str)
    {
        set_model(state, model);
    }
    let model = state
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let base = SessionMessageMetadata {
        timestamp,
        model: model.clone(),
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let mut result = Vec::new();
    let mut text = Vec::new();
    if let Some(content) = message.get("content") {
        match content {
            Value::String(content) if !content.trim().is_empty() => {
                text.push(content.clone());
            }
            Value::Array(blocks) => {
                for (index, block) in blocks.iter().enumerate() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(value) = block.get("text").and_then(Value::as_str) {
                                text.push(value.to_owned());
                            }
                        }
                        Some("thinking") => {
                            if let Some(value) = block.get("thinking").and_then(Value::as_str) {
                                push(
                                    &mut result,
                                    format!("cline-{id}:thinking:{index:04}"),
                                    CliSessionMessageRole::Assistant,
                                    value.to_owned(),
                                    SessionMessageMetadata {
                                        kind: SessionMessageKind::Thinking,
                                        ..base.clone()
                                    },
                                );
                            }
                        }
                        Some("tool_use") => {
                            let name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("工具");
                            let call_id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .or_else(|| Some(format!("cline-{id}-call-{index}")));
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            let mut metadata = presentation::tool_call(name, &input, call_id);
                            metadata.timestamp = base.timestamp.clone();
                            metadata.model = base.model.clone();
                            metadata.group_id = base.group_id.clone();
                            push(
                                &mut result,
                                format!("cline-{id}:call:{index:04}"),
                                CliSessionMessageRole::Tool,
                                json_text(&input),
                                metadata,
                            );
                        }
                        Some("tool_result") => {
                            let call_id = block
                                .get("tool_use_id")
                                .or_else(|| block.get("tool_call_id"))
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .or_else(|| Some(format!("cline-{id}-result-{index}")));
                            let content = block.get("content").unwrap_or(&Value::Null);
                            let mut metadata = presentation::tool_result(content, call_id.clone());
                            metadata.timestamp = base.timestamp.clone();
                            metadata.model = base.model.clone();
                            metadata.group_id = base.group_id.clone();
                            metadata.tool_kind = Some(SessionToolKind::Other);
                            push(
                                &mut result,
                                format!("cline-{id}:result:{index:04}"),
                                CliSessionMessageRole::Tool,
                                json_text(content),
                                metadata,
                            );
                        }
                        Some("image") => {
                            if let Some(reference) = image_reference(block) {
                                push(
                                    &mut result,
                                    format!("cline-{id}:image:{index:04}"),
                                    CliSessionMessageRole::User,
                                    reference,
                                    SessionMessageMetadata {
                                        kind: SessionMessageKind::Image,
                                        ..base.clone()
                                    },
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        let role = if role == "user" {
            CliSessionMessageRole::User
        } else if role == "tool" {
            CliSessionMessageRole::Tool
        } else {
            CliSessionMessageRole::Assistant
        };
        push(
            &mut result,
            format!("cline-{id}"),
            role,
            text.join("\n"),
            SessionMessageMetadata {
                kind: SessionMessageKind::Text,
                ..base.clone()
            },
        );
    }
    if let Some(metrics) = message
        .get("metrics")
        .or_else(|| message.get("usage"))
        .or_else(|| value.pointer("/metrics"))
    {
        let tokens = SessionTokenCounts {
            input_tokens: number(metrics, &["inputTokens", "input_tokens", "promptTokens"]),
            output_tokens: number(metrics, &["outputTokens", "output_tokens", "completionTokens"]),
            cache_read_tokens: number(metrics, &["cacheReadTokens", "cache_read_tokens"]),
            cache_write_tokens: number(metrics, &["cacheWriteTokens", "cache_write_tokens"]),
            reasoning_tokens: number(metrics, &["reasoningTokens", "reasoning_tokens"]),
        };
        if tokens != SessionTokenCounts::default() {
            result.push(usage::record(
                format!("cline-usage-{id}"),
                model.as_deref(),
                tokens,
                false,
                false,
                Some(&format!("cline-{id}")),
            ));
        }
    }
    result
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

fn number(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
}

fn image_reference(block: &Value) -> Option<String> {
    if let Some(url) = block.get("url").and_then(Value::as_str) {
        return Some(url.to_owned());
    }
    block
        .pointer("/source/url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            block
                .pointer("/source/media_type")
                .or_else(|| block.pointer("/source/mediaType"))
                .and_then(Value::as_str)
                .map(|mime| format!("data:{mime};base64,[inline image]"))
        })
}

fn record_time(value: &Value) -> Option<String> {
    if let Some(number) = value.as_i64() {
        return crate::services::cli_sessions::timestamp_from_value(Some(number), true);
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
