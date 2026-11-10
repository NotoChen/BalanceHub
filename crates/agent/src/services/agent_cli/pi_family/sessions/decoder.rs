//! Pi-family JSONL transcript decoder.
//!
//! Entries are append-only and self-contained. `message` entries wrap an
//! AgentMessage; `usage` entries carry token totals outside the model context.
//! Unknown entry types are tolerated so a newer Pi release does not make older
//! history unreadable.

use crate::{
    models::{
        SessionMessageKind, SessionMessageMetadata, SessionTokenCounts, SessionToolKind,
        SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, normalize_timestamp, presentation, timestamp_from_value, usage},
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
    match value.get("type").and_then(Value::as_str) {
        Some("message") => decode_message(&value, sequence, state, &mut result),
        Some("usage") => decode_usage(&value, sequence, &mut result),
        Some("model_change") => {
            if let Some(model) = string_field(&value, &["modelId", "model"]) {
                set_model(state, model);
            }
        }
        Some("custom_message") | Some("custom") => decode_entry_text(&value, sequence, &mut result),
        Some("compaction") | Some("branch_summary") => {
            decode_entry_summary(&value, sequence, &mut result)
        }
        Some("session_info") => {
            if let Some(model) = string_field(&value, &["modelId", "model"]) {
                set_model(state, model);
            }
        }
        _ => {}
    }
    result
}

fn decode_entry_text(entry: &Value, sequence: u64, result: &mut Vec<SessionTranscriptMutation>) {
    let text = entry
        .pointer("/message/content")
        .and_then(|value| content_text(Some(value)))
        .or_else(|| {
            entry
                .get("content")
                .and_then(|value| content_text(Some(value)))
        });
    let Some(text) = text else {
        return;
    };
    let id = entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("entry-{sequence}"));
    let base = SessionMessageMetadata {
        timestamp: message_timestamp(entry, entry),
        group_id: Some(id.clone()),
        injected: true,
        ..Default::default()
    };
    push_text(
        result,
        format!("pi-{id}"),
        crate::models::CliSessionMessageRole::Assistant,
        text,
        base,
        SessionMessageKind::Event,
    );
}

fn decode_entry_summary(entry: &Value, sequence: u64, result: &mut Vec<SessionTranscriptMutation>) {
    let Some(text) = string_field(entry, &["summary"]) else {
        return;
    };
    let id = entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("entry-{sequence}"));
    push_text(
        result,
        format!("pi-{id}"),
        crate::models::CliSessionMessageRole::Assistant,
        text,
        SessionMessageMetadata {
            timestamp: message_timestamp(entry, entry),
            group_id: Some(id),
            ..Default::default()
        },
        SessionMessageKind::Event,
    );
}

fn decode_message(
    entry: &Value,
    sequence: u64,
    state: &mut Value,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let Some(message) = entry.get("message") else {
        return;
    };
    let entry_id = entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("entry-{sequence}"));
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let timestamp = message_timestamp(entry, message);
    let model = string_field(message, &["model"]).or_else(|| state_model(state).map(str::to_owned));
    if let Some(model) = &model {
        set_model(state, model.clone());
    }
    let base = SessionMessageMetadata {
        timestamp,
        model,
        group_id: Some(entry_id.clone()),
        ..Default::default()
    };
    match role {
        "user" => {
            if let Some(text) = content_text(message.get("content")) {
                push_text(
                    result,
                    format!("pi-{entry_id}"),
                    crate::models::CliSessionMessageRole::User,
                    text,
                    base,
                    SessionMessageKind::Text,
                );
            }
        }
        "assistant" => decode_assistant(&entry_id, message, base, result),
        "toolResult" => decode_tool_result(&entry_id, message, base, result),
        "bashExecution" => decode_bash(&entry_id, message, base, result),
        "custom_message" | "custom" => {
            if let Some(text) = content_text(message.get("content")) {
                push_text(
                    result,
                    format!("pi-{entry_id}"),
                    crate::models::CliSessionMessageRole::Assistant,
                    text,
                    base,
                    SessionMessageKind::Event,
                );
            }
        }
        "branchSummary" | "compactionSummary" => {
            if let Some(text) = string_field(message, &["summary"]) {
                push_text(
                    result,
                    format!("pi-{entry_id}"),
                    crate::models::CliSessionMessageRole::Assistant,
                    text,
                    base,
                    SessionMessageKind::Event,
                );
            }
        }
        _ => {}
    }
    if let Some(usage_value) = message.get("usage") {
        push_usage(
            result,
            format!("pi-usage-{entry_id}"),
            model_ref(message),
            usage_value,
            Some(format!("pi-{entry_id}")),
        );
    }
}

fn decode_assistant(
    entry_id: &str,
    message: &Value,
    base: SessionMessageMetadata,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let Some(content) = message.get("content") else {
        return;
    };
    if let Some(text) = content.as_str() {
        push_text(
            result,
            format!("pi-{entry_id}"),
            crate::models::CliSessionMessageRole::Assistant,
            text.to_owned(),
            base,
            SessionMessageKind::Text,
        );
        return;
    }
    let Some(blocks) = content.as_array() else {
        return;
    };
    let mut text = Vec::new();
    let mut part = 0usize;
    for block in blocks {
        part += 1;
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push(value.to_owned());
                }
            }
            Some("thinking") => {
                if let Some(value) = block
                    .get("thinking")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                {
                    push_text(
                        result,
                        format!("pi-{entry_id}:thinking:{part:06}"),
                        crate::models::CliSessionMessageRole::Assistant,
                        value.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                        SessionMessageKind::Thinking,
                    );
                }
            }
            Some("toolCall") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("工具");
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some(format!("pi-{entry_id}-tool-{part}")));
                let arguments = block
                    .get("arguments")
                    .or_else(|| block.get("input"))
                    .unwrap_or(&Value::Null);
                let mut metadata = presentation::tool_call(name, arguments, call_id);
                metadata.timestamp = base.timestamp.clone();
                metadata.model = base.model.clone();
                metadata.group_id = base.group_id.clone();
                push_text(
                    result,
                    format!("pi-{entry_id}:tool:{part:06}"),
                    crate::models::CliSessionMessageRole::Tool,
                    json_text(arguments),
                    metadata,
                    SessionMessageKind::ToolCall,
                );
            }
            Some("image") => {
                if let (Some(media), Some(data)) = (
                    block.get("mimeType").and_then(Value::as_str),
                    block.get("data").and_then(Value::as_str),
                ) {
                    push_text(
                        result,
                        format!("pi-{entry_id}:image:{part:06}"),
                        crate::models::CliSessionMessageRole::Assistant,
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
    if !text.is_empty() {
        push_text(
            result,
            format!("pi-{entry_id}"),
            crate::models::CliSessionMessageRole::Assistant,
            text.join("\n"),
            base,
            SessionMessageKind::Text,
        );
    }
}

fn decode_tool_result(
    entry_id: &str,
    message: &Value,
    base: SessionMessageMetadata,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let content = message.get("content");
    let call_id = message
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut metadata = presentation::tool_result(message, call_id);
    metadata.timestamp = base.timestamp.clone();
    metadata.model = base.model.clone();
    metadata.group_id = base.group_id.clone();
    metadata.tool_name = message
        .get("toolName")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let text = content_text(content).unwrap_or_default();
    if !text.trim().is_empty() {
        push_text(
            result,
            format!("pi-{entry_id}:result"),
            crate::models::CliSessionMessageRole::Tool,
            text,
            metadata,
            SessionMessageKind::ToolResult,
        );
    }
    if let Some(blocks) = content.and_then(Value::as_array) {
        for (part, block) in blocks.iter().enumerate() {
            if block.get("type").and_then(Value::as_str) != Some("image") {
                continue;
            }
            if let (Some(media), Some(data)) = (
                block.get("mimeType").and_then(Value::as_str),
                block.get("data").and_then(Value::as_str),
            ) {
                push_text(
                    result,
                    format!("pi-{entry_id}:image:{:06}", part + 1),
                    crate::models::CliSessionMessageRole::Tool,
                    format!("data:{media};base64,{data}"),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Image,
                        ..base.clone()
                    },
                    SessionMessageKind::Image,
                );
            }
        }
    }
}

fn decode_bash(
    entry_id: &str,
    message: &Value,
    base: SessionMessageMetadata,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let command = message
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = message
        .get("output")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let exit_code = message
        .get("exitCode")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok());
    let cancelled = message
        .get("cancelled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let status = if cancelled {
        Some(SessionToolStatus::Cancelled)
    } else if exit_code.is_some_and(|code| code != 0) {
        Some(SessionToolStatus::Failed)
    } else if exit_code.is_some() {
        Some(SessionToolStatus::Succeeded)
    } else {
        None
    };
    let metadata = SessionMessageMetadata {
        kind: SessionMessageKind::ToolResult,
        timestamp: base.timestamp,
        model: base.model,
        group_id: base.group_id,
        tool_name: Some("bash".into()),
        tool_kind: Some(SessionToolKind::Shell),
        call_id: Some(format!("pi-{entry_id}-bash")),
        status,
        exit_code,
        ..Default::default()
    };
    let content = format!("$ {command}\n{output}");
    push_text(
        result,
        format!("pi-{entry_id}:bash"),
        crate::models::CliSessionMessageRole::Tool,
        content,
        metadata,
        SessionMessageKind::ToolResult,
    );
}

fn decode_usage(entry: &Value, sequence: u64, result: &mut Vec<SessionTranscriptMutation>) {
    let Some(usage_value) = entry.get("usage") else {
        return;
    };
    let id = entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("usage-{sequence}"));
    push_usage(
        result,
        format!("pi-usage-{id}"),
        entry.get("model").and_then(Value::as_str),
        usage_value,
        None,
    );
}

fn push_usage(
    result: &mut Vec<SessionTranscriptMutation>,
    id: String,
    model: Option<&str>,
    usage_value: &Value,
    message_id: Option<String>,
) {
    result.push(usage::record(
        id,
        model,
        SessionTokenCounts {
            input_tokens: number(usage_value, "input"),
            output_tokens: number(usage_value, "output"),
            cache_read_tokens: number(usage_value, "cacheRead"),
            cache_write_tokens: number(usage_value, "cacheWrite"),
            reasoning_tokens: None,
        },
        false,
        false,
        message_id.as_deref(),
    ));
}

fn push_text(
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

fn message_timestamp(entry: &Value, message: &Value) -> Option<String> {
    message
        .get("timestamp")
        .and_then(Value::as_i64)
        .and_then(|value| timestamp_from_value(Some(value), true))
        .or_else(|| {
            entry
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|value| normalize_timestamp(Some(value)))
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

pub(super) fn message_text(message: &Value) -> Option<String> {
    content_text(message.get("content"))
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

fn number(value: &Value, name: &str) -> Option<u64> {
    value.get(name).and_then(Value::as_u64)
}

fn model_ref(message: &Value) -> Option<&str> {
    message.get("model").and_then(Value::as_str)
}

fn state_model(state: &Value) -> Option<&str> {
    state.get("model").and_then(Value::as_str)
}

fn set_model(state: &mut Value, model: String) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    if let Some(object) = state.as_object_mut() {
        object.insert("model".into(), Value::String(model));
    }
}
