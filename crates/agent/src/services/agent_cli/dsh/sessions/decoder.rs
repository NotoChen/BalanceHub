//! dsh session-log decoder.
//!
//! Log events carry the provider-neutral conversation messages, plus separate
//! tool-call and tool-result events. Both views use the same call id for their
//! transcript row so a repeated Put replaces instead of duplicating.

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
    let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    let data = value.get("data").unwrap_or(&Value::Null);
    let timestamp = value
        .get("time")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)));
    match kind {
        "session" => Vec::new(),
        "session/title" => {
            if let Some(title) = data
                .get("title")
                .or_else(|| data.get("name"))
                .and_then(Value::as_str)
            {
                set_state(state, "title", title);
            }
            Vec::new()
        }
        "user/message" => {
            let message = data.get("message").unwrap_or(data);
            decode_message(message, sequence, timestamp, state)
        }
        "assistant/message" => {
            let message = data.get("message").unwrap_or(data);
            let mut result = decode_message(message, sequence, timestamp, state);
            if let Some(usage_value) = data.get("usage") {
                let message_id = message
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("dsh-{sequence}"));
                result.push(usage::record(
                    format!("dsh-usage-{message_id}"),
                    state.get("model").and_then(Value::as_str),
                    SessionTokenCounts {
                        input_tokens: usage::number(usage_value, "inputTokens"),
                        output_tokens: usage::number(usage_value, "outputTokens"),
                        cache_read_tokens: usage::number(usage_value, "cacheReadTokens"),
                        cache_write_tokens: usage::number(usage_value, "cacheWriteTokens"),
                        reasoning_tokens: usage::number(usage_value, "reasoningTokens"),
                    },
                    false,
                    false,
                    Some(&message_id),
                ));
            }
            result
        }
        "tool/call" => decode_tool_call(data, sequence, timestamp, state),
        "tool/result" => {
            let message = data.get("message").unwrap_or(data);
            decode_message(message, sequence, timestamp, state)
        }
        kind if kind.contains("compact") => decode_compaction(data, sequence, timestamp),
        _ => Vec::new(),
    }
}

fn decode_message(
    message: &Value,
    sequence: u64,
    timestamp: Option<String>,
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let source_kind = message
        .pointer("/source/kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = match message.get("role").and_then(Value::as_str) {
        Some("assistant") => CliSessionMessageRole::Assistant,
        Some("system") => return Vec::new(),
        _ if source_kind == "tool" => CliSessionMessageRole::Tool,
        _ => CliSessionMessageRole::User,
    };
    if let Some(model) = message
        .pointer("/source/model")
        .or_else(|| message.get("model"))
        .and_then(Value::as_str)
    {
        set_state(state, "model", model);
    }
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("dsh-{sequence}"));
    let mut base = SessionMessageMetadata {
        timestamp,
        model: state
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        group_id: Some(id.clone()),
        injected: source_kind == "plugin",
        ..Default::default()
    };
    let mut result = Vec::new();
    let mut text = Vec::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for (index, block) in blocks.iter().enumerate() {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        text.push(value.to_owned());
                    }
                }
                Some("reasoning") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        push(
                            &mut result,
                            format!("dsh-{id}:reasoning:{index:04}"),
                            CliSessionMessageRole::Assistant,
                            value.to_owned(),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Thinking,
                                ..base.clone()
                            },
                        );
                    }
                }
                Some("tool-call") => {
                    let call_id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| Some(format!("dsh-{id}-call-{index}")));
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("工具");
                    let arguments = block.get("arguments").cloned().unwrap_or(Value::Null);
                    push_tool_call(&mut result, call_id, name, &arguments, &base);
                }
                Some("tool-result") => {
                    push_tool_result(&mut result, block, &base);
                }
                Some("image") => {
                    if let Some(reference) = image_reference(block) {
                        push(
                            &mut result,
                            format!("dsh-{id}:image:{index:04}"),
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
    if !text.is_empty() {
        let role = if role == CliSessionMessageRole::Tool {
            CliSessionMessageRole::Tool
        } else {
            base.injected = base.injected && role == CliSessionMessageRole::User;
            role
        };
        push(
            &mut result,
            format!("dsh-{id}"),
            role,
            text.join("\n"),
            SessionMessageMetadata {
                kind: SessionMessageKind::Text,
                ..base
            },
        );
    }
    result
}

fn decode_tool_call(
    data: &Value,
    sequence: u64,
    timestamp: Option<String>,
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let call_id = data
        .get("callId")
        .or_else(|| data.get("call_id"))
        .or_else(|| data.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("dsh-call-{sequence}"));
    let name = data.get("name").and_then(Value::as_str).unwrap_or("工具");
    let arguments = data.get("arguments").cloned().unwrap_or(Value::Null);
    let base = SessionMessageMetadata {
        timestamp,
        model: state
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        group_id: Some(call_id.clone()),
        ..Default::default()
    };
    let mut result = Vec::new();
    push_tool_call(&mut result, Some(call_id), name, &arguments, &base);
    result
}

fn push_tool_call(
    result: &mut Vec<SessionTranscriptMutation>,
    call_id: Option<String>,
    name: &str,
    arguments: &Value,
    base: &SessionMessageMetadata,
) {
    let id = call_id
        .as_deref()
        .map(|call_id| format!("dsh-call-{call_id}"))
        .unwrap_or_else(|| format!("dsh-call-{}", base.group_id.as_deref().unwrap_or("?")));
    let mut metadata = presentation::tool_call(name, arguments, call_id);
    metadata.timestamp = base.timestamp.clone();
    metadata.model = base.model.clone();
    metadata.group_id = base.group_id.clone();
    push(
        result,
        id,
        CliSessionMessageRole::Tool,
        json_text(arguments),
        metadata,
    );
}

fn push_tool_result(
    result: &mut Vec<SessionTranscriptMutation>,
    block: &Value,
    base: &SessionMessageMetadata,
) {
    let call_id = block
        .get("toolCallId")
        .or_else(|| block.get("tool_call_id"))
        .or_else(|| block.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let id = call_id
        .as_deref()
        .map(|call_id| format!("dsh-result-{call_id}"))
        .unwrap_or_else(|| format!("dsh-result-{}", base.group_id.as_deref().unwrap_or("?")));
    let content = block.get("content").unwrap_or(&Value::Null);
    let mut metadata = presentation::tool_result(block, call_id.clone());
    metadata.timestamp = base.timestamp.clone();
    metadata.model = base.model.clone();
    metadata.group_id = base.group_id.clone();
    metadata.tool_kind = Some(SessionToolKind::Other);
    if metadata.status.is_none() {
        metadata.status = Some(if block.get("isError").and_then(Value::as_bool) == Some(true) {
            SessionToolStatus::Failed
        } else {
            SessionToolStatus::Succeeded
        });
    }
    push(
        result,
        id,
        CliSessionMessageRole::Tool,
        json_text(content),
        metadata,
    );
}

fn decode_compaction(
    data: &Value,
    sequence: u64,
    timestamp: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    let text = data
        .get("summary")
        .or_else(|| data.get("text"))
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty());
    text.map(|text| {
        vec![SessionTranscriptMutation::Put {
            message: SessionTranscriptMessage {
                id: format!("dsh-compaction-{sequence}"),
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

fn image_reference(block: &Value) -> Option<String> {
    let attachment = block.get("attachment").unwrap_or(block);
    for pointer in ["/url", "/path", "/id"] {
        if let Some(value) = attachment.pointer(pointer).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    attachment
        .get("mediaType")
        .or_else(|| attachment.get("mimeType"))
        .and_then(Value::as_str)
        .map(|mime| format!("data:{mime};base64,[inline image]"))
}

fn set_state(state: &mut Value, key: &str, value: &str) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    if let Some(object) = state.as_object_mut() {
        object.insert(key.into(), Value::String(value.to_owned()));
    }
}
