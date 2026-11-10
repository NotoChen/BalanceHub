//! jcode StoredMessage decoder.
//!
//! Snapshot `messages` elements and journal `append_messages` elements share
//! the same StoredMessage shape. Content blocks are internally tagged with
//! `type` and use the provider-neutral jcode vocabulary.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolKind, SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, presentation, usage},
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
    let mut result = Vec::new();
    if let Some(messages) = value.get("append_messages").and_then(Value::as_array) {
        for (index, message) in messages.iter().enumerate() {
            decode_message(message, sequence, index, state, &mut result);
        }
        return result;
    }
    if value.get("role").is_some() && value.get("content").is_some() {
        decode_message(&value, sequence, 0, state, &mut result);
    }
    result
}

fn decode_message(
    message: &Value,
    sequence: u64,
    index: usize,
    state: &mut Value,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("jcode-{sequence}-{index}"));
    let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
    let timestamp = message
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| crate::services::cli_sessions::normalize_timestamp(Some(value)));
    let base = SessionMessageMetadata {
        timestamp,
        model: state.get("model").and_then(Value::as_str).map(str::to_owned),
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let display_role = message.get("display_role").and_then(Value::as_str);
    let mut text = Vec::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for (part_index, block) in blocks.iter().enumerate() {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        text.push(value.to_owned());
                    }
                }
                Some("reasoning") | Some("reasoning_trace") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        push_text(
                            result,
                            format!("jcode-{id}:thinking:{part_index:06}"),
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
                Some("anthropic_thinking") => {
                    if let Some(value) = block.get("thinking").and_then(Value::as_str) {
                        push_text(
                            result,
                            format!("jcode-{id}:thinking:{part_index:06}"),
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
                Some("open_ai_reasoning") => {
                    if let Some(items) = block.get("summary").and_then(Value::as_array) {
                        let value = items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !value.trim().is_empty() {
                            push_text(
                                result,
                                format!("jcode-{id}:thinking:{part_index:06}"),
                                CliSessionMessageRole::Assistant,
                                value,
                                SessionMessageMetadata {
                                    kind: SessionMessageKind::Thinking,
                                    ..base.clone()
                                },
                                SessionMessageKind::Thinking,
                            );
                        }
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("工具");
                    let call_id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| Some(format!("jcode-{id}-tool-{part_index}")));
                    let arguments = block.get("input").unwrap_or(&Value::Null);
                    let mut metadata = presentation::tool_call(name, arguments, call_id);
                    metadata.timestamp = base.timestamp.clone();
                    metadata.model = base.model.clone();
                    metadata.group_id = base.group_id.clone();
                    push_text(
                        result,
                        format!("jcode-{id}:tool:{part_index:06}"),
                        CliSessionMessageRole::Tool,
                        json_text(arguments),
                        metadata,
                        SessionMessageKind::ToolCall,
                    );
                }
                Some("tool_result") => {
                    let call_id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let content = block
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let mut metadata = presentation::tool_result(block, call_id);
                    metadata.timestamp = base.timestamp.clone();
                    metadata.group_id = base.group_id.clone();
                    metadata.tool_kind = Some(SessionToolKind::Other);
                    metadata.status = block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .map(|failed| {
                            if failed {
                                SessionToolStatus::Failed
                            } else {
                                SessionToolStatus::Succeeded
                            }
                        });
                    push_text(
                        result,
                        format!("jcode-{id}:result:{part_index:06}"),
                        CliSessionMessageRole::Tool,
                        content.to_owned(),
                        metadata,
                        SessionMessageKind::ToolResult,
                    );
                }
                Some("image") => {
                    if let (Some(media), Some(data)) = (
                        block.get("media_type").and_then(Value::as_str),
                        block.get("data").and_then(Value::as_str),
                    ) {
                        push_text(
                            result,
                            format!("jcode-{id}:image:{part_index:06}"),
                            CliSessionMessageRole::User,
                            format!("data:{media};base64,{data}"),
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
    }
    if text.iter().any(|value| !value.trim().is_empty()) {
        let role = match role {
            "User" | "user" => CliSessionMessageRole::User,
            _ => CliSessionMessageRole::Assistant,
        };
        let kind = if display_role.is_some() {
            SessionMessageKind::Event
        } else {
            SessionMessageKind::Text
        };
        push_text(
            result,
            format!("jcode-{id}"),
            role,
            text.join("\n"),
            base.clone(),
            kind,
        );
    }
    if let Some(usage_value) = message.get("token_usage") {
        result.push(usage::record(
            format!("jcode-usage-{id}"),
            base.model.as_deref(),
            SessionTokenCounts {
                input_tokens: usage_value.get("input_tokens").and_then(Value::as_u64),
                output_tokens: usage_value.get("output_tokens").and_then(Value::as_u64),
                cache_read_tokens: usage_value
                    .get("cache_read_input_tokens")
                    .and_then(Value::as_u64),
                cache_write_tokens: usage_value
                    .get("cache_creation_input_tokens")
                    .and_then(Value::as_u64),
                reasoning_tokens: None,
            },
            false,
            false,
            Some(&format!("jcode-{id}")),
        ));
    }
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
