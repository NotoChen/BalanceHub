//! Qwen Code ChatRecord decoder.
//!
//! Qwen reuses Gemini's `Content` shape: `message.parts` carries text, thinking,
//! `functionCall` and `functionResponse` blocks. Older files may store the
//! message as a bare part array.

use crate::{
    models::{SessionMessageKind, SessionMessageMetadata, SessionTokenCounts},
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{
            json_text, normalize_timestamp, presentation, timestamp_from_value, usage,
        },
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
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let record_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if record_type == "system" {
        return result;
    }
    let Some(message) = value.get("message") else {
        return result;
    };
    let entry_id = value
        .get("uuid")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("entry-{sequence}"));
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .or(match record_type {
            "user" => Some("user"),
            "assistant" => Some("model"),
            "tool_result" => Some("tool"),
            _ => None,
        });
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)))
        .or_else(|| {
            value
                .get("timestamp")
                .and_then(Value::as_i64)
                .and_then(|value| timestamp_from_value(Some(value), false))
        });
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let base = SessionMessageMetadata {
        timestamp,
        model: model.clone(),
        group_id: Some(entry_id.clone()),
        ..Default::default()
    };
    let parts = message
        .get("parts")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| message.as_array().cloned())
        .unwrap_or_default();
    let mut text = Vec::new();
    let mut part = 0usize;
    for block in &parts {
        part += 1;
        match block.get("type").and_then(Value::as_str) {
            Some("functionCall") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("工具");
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some(format!("qwen-{entry_id}-tool-{part}")));
                let arguments = block.get("args").unwrap_or(&Value::Null);
                let mut metadata = presentation::tool_call(name, arguments, call_id);
                metadata.timestamp = base.timestamp.clone();
                metadata.model = base.model.clone();
                metadata.group_id = base.group_id.clone();
                push(
                    &mut result,
                    format!("qwen-{entry_id}:tool:{part:06}"),
                    role_for_tool(role),
                    json_text(arguments),
                    metadata,
                    SessionMessageKind::ToolCall,
                );
            }
            Some("functionResponse") => {
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some(format!("qwen-{entry_id}-tool-{part}")));
                let response = block
                    .get("response")
                    .or_else(|| block.get("result"))
                    .unwrap_or(&Value::Null);
                let mut metadata = presentation::tool_result(response, call_id);
                metadata.timestamp = base.timestamp.clone();
                metadata.model = base.model.clone();
                metadata.group_id = base.group_id.clone();
                metadata.tool_name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                push(
                    &mut result,
                    format!("qwen-{entry_id}:result:{part:06}"),
                    crate::models::CliSessionMessageRole::Tool,
                    json_text(response),
                    metadata,
                    SessionMessageKind::ToolResult,
                );
            }
            Some("image") | Some("inlineData") => {
                let data = block
                    .get("inlineData")
                    .or_else(|| block.get("inline_data"))
                    .unwrap_or(block);
                if let (Some(media), Some(bytes)) = (
                    data.get("mimeType")
                        .or_else(|| data.get("mime_type"))
                        .and_then(Value::as_str),
                    data.get("data").and_then(Value::as_str),
                ) {
                    push(
                        &mut result,
                        format!("qwen-{entry_id}:image:{part:06}"),
                        role_for_user(role),
                        format!("data:{media};base64,{bytes}"),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Image,
                            ..base.clone()
                        },
                        SessionMessageKind::Image,
                    );
                }
            }
            Some("thinking") => {
                if let Some(value) = block.get("thinking").and_then(Value::as_str) {
                    push(
                        &mut result,
                        format!("qwen-{entry_id}:thinking:{part:06}"),
                        role_for_user(role),
                        value.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                        SessionMessageKind::Thinking,
                    );
                }
            }
            _ => {
                if block.get("thought").and_then(Value::as_bool) == Some(true) {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        push(
                            &mut result,
                            format!("qwen-{entry_id}:thinking:{part:06}"),
                            role_for_user(role),
                            value.to_owned(),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Thinking,
                                ..base.clone()
                            },
                            SessionMessageKind::Thinking,
                        );
                    }
                } else if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push(value.to_owned());
                }
            }
        }
    }
    if !text.is_empty() {
        let text_role = role_for_user(role);
        push(
            &mut result,
            format!("qwen-{entry_id}"),
            text_role,
            text.join("\n"),
            base,
            SessionMessageKind::Text,
        );
    } else if let Some(value) = message.get("content").and_then(Value::as_str) {
        push(
            &mut result,
            format!("qwen-{entry_id}"),
            role_for_user(role),
            value.to_owned(),
            base,
            SessionMessageKind::Text,
        );
    }
    if let Some(usage_value) = value.get("usageMetadata") {
        result.push(usage::record(
            format!("qwen-usage-{entry_id}"),
            model.as_deref(),
            SessionTokenCounts {
                input_tokens: number(usage_value, "promptTokenCount"),
                output_tokens: number(usage_value, "candidatesTokenCount")
                    .map(|value| {
                        value.saturating_add(number(usage_value, "thoughtsTokenCount").unwrap_or_default())
                    }),
                cache_read_tokens: number(usage_value, "cachedContentTokenCount"),
                cache_write_tokens: None,
                reasoning_tokens: number(usage_value, "thoughtsTokenCount"),
            },
            false,
            false,
            Some(&format!("qwen-{entry_id}")),
        ));
    }
    result
}

fn role_for_user(role: Option<&str>) -> crate::models::CliSessionMessageRole {
    match role {
        Some("user") => crate::models::CliSessionMessageRole::User,
        Some("tool") | Some("tool_result") => crate::models::CliSessionMessageRole::Tool,
        _ => crate::models::CliSessionMessageRole::Assistant,
    }
}

fn role_for_tool(role: Option<&str>) -> crate::models::CliSessionMessageRole {
    match role {
        Some("user") => crate::models::CliSessionMessageRole::User,
        _ => crate::models::CliSessionMessageRole::Tool,
    }
}

fn push(
    result: &mut Vec<SessionTranscriptMutation>,
    id: String,
    role: crate::models::CliSessionMessageRole,
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

fn number(value: &Value, name: &str) -> Option<u64> {
    value.get(name).and_then(Value::as_u64)
}
