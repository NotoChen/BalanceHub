//! MiniMax Code display-row decoder.
//!
//! `local_runtime_message_rows.data_json` is the UI-visible projection of the
//! canonical conversation: user text, assistant text/thinking/tool calls and
//! the tool output attached to each call. System events (`msg_type = 3`) are
//! runtime bookkeeping and never appear in the conversation view, so they are
//! skipped here as well.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, presentation, timestamp_from_value, usage},
    },
};
use serde_json::Value;
use std::path::Path;

const MSG_TYPE_SYSTEM_EVENT: i64 = 3;

pub(super) fn decode(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    if value.get("msg_type").and_then(Value::as_i64) == Some(MSG_TYPE_SYSTEM_EVENT) {
        return Vec::new();
    }
    let id = value
        .get("msg_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("minimax-row-{sequence}"));
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_i64)
        .and_then(|value| timestamp_from_value(Some(value), true));
    let turn_id = value
        .get("turnId")
        .or_else(|| value.get("turn_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let model = value
        .pointer("/context_usage_telemetry/model")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    match value.get("role").and_then(Value::as_str) {
        Some("user") => decode_user(&value, &id, timestamp, turn_id),
        Some("assistant") => decode_assistant(&value, &id, timestamp, turn_id, model),
        Some("tool") => decode_tool_row(&value, &id, timestamp, turn_id),
        _ => Vec::new(),
    }
}

fn decode_user(
    value: &Value,
    id: &str,
    timestamp: Option<String>,
    turn_id: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    let Some(content) = content_text(value.get("msg_content")) else {
        return Vec::new();
    };
    vec![mutation(
        format!("minimax-{id}"),
        CliSessionMessageRole::User,
        content,
        SessionMessageMetadata {
            kind: SessionMessageKind::Text,
            timestamp,
            turn_id: turn_id.clone(),
            group_id: turn_id,
            ..Default::default()
        },
    )]
}

fn decode_assistant(
    value: &Value,
    id: &str,
    timestamp: Option<String>,
    turn_id: Option<String>,
    model: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    let mut result = Vec::new();
    let mut text_id = None;
    if let Some(content) = content_text(value.get("thinking_content")) {
        result.push(mutation(
            format!("minimax-{id}:thinking"),
            CliSessionMessageRole::Assistant,
            content,
            SessionMessageMetadata {
                kind: SessionMessageKind::Thinking,
                timestamp: timestamp.clone(),
                model: model.clone(),
                turn_id: turn_id.clone(),
                group_id: turn_id.clone().or_else(|| Some(id.to_owned())),
                ..Default::default()
            },
        ));
    }
    if let Some(content) = content_text(value.get("msg_content")) {
        let message_id = format!("minimax-{id}");
        result.push(mutation(
            message_id.clone(),
            CliSessionMessageRole::Assistant,
            content,
            SessionMessageMetadata {
                kind: SessionMessageKind::Text,
                timestamp: timestamp.clone(),
                model: model.clone(),
                turn_id: turn_id.clone(),
                group_id: turn_id.clone().or_else(|| Some(id.to_owned())),
                ..Default::default()
            },
        ));
        text_id = Some(message_id);
    }
    if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            decode_tool_call(
                call,
                timestamp.clone(),
                turn_id.clone().or_else(|| Some(id.to_owned())),
                model.clone(),
                &mut result,
            );
        }
    }
    if let Some(counts) = token_counts(value.get("usage")) {
        result.push(usage::record(
            format!("minimax-usage-{id}"),
            model.as_deref(),
            counts,
            false,
            false,
            text_id.as_deref(),
        ));
    }
    result
}

fn decode_tool_call(
    call: &Value,
    timestamp: Option<String>,
    group_id: Option<String>,
    model: Option<String>,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let Some(call_id) = call
        .get("tool_call_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return;
    };
    let name = call
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("工具");
    let arguments = parse_payload(call.get("tool_call_args")).unwrap_or(Value::Null);
    let mut metadata = presentation::tool_call(name, &arguments, Some(call_id.to_owned()));
    metadata.timestamp = timestamp.clone();
    metadata.model = model;
    metadata.turn_id = group_id.clone();
    metadata.group_id = group_id;
    metadata.status = call_status(call);
    result.push(mutation(
        format!("minimax-call-{call_id}"),
        CliSessionMessageRole::Tool,
        json_text(&arguments),
        metadata,
    ));
    let Some(output) = parse_payload(call.get("tool_call_result_data")) else {
        return;
    };
    if output.is_null() {
        return;
    }
    let mut metadata = presentation::tool_result(&output, Some(call_id.to_owned()));
    metadata.timestamp = timestamp;
    metadata.call_id = Some(call_id.to_owned());
    if let Some(status) = call_status(call) {
        metadata.status = Some(status);
    }
    if let Some(code) = exit_code(&output) {
        metadata.exit_code = Some(code);
    }
    result.push(mutation(
        format!("minimax-result-{call_id}"),
        CliSessionMessageRole::Tool,
        result_text(&output),
        metadata,
    ));
}

fn decode_tool_row(
    value: &Value,
    id: &str,
    timestamp: Option<String>,
    turn_id: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    let Some(content) = content_text(value.get("msg_content")) else {
        return Vec::new();
    };
    let call_id = value
        .get("tool_call_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    let mut metadata = presentation::tool_result(value, call_id.clone());
    metadata.timestamp = timestamp;
    metadata.turn_id = turn_id.clone();
    metadata.group_id = turn_id;
    metadata.call_id = call_id;
    vec![mutation(
        format!("minimax-result-row-{id}"),
        CliSessionMessageRole::Tool,
        content,
        metadata,
    )]
}

fn call_status(call: &Value) -> Option<SessionToolStatus> {
    match call.get("tool_call_status") {
        Some(Value::Number(number)) => match number.as_i64() {
            Some(2) => Some(SessionToolStatus::Succeeded),
            Some(3) => Some(SessionToolStatus::Failed),
            Some(1 | 4 | 5) => Some(SessionToolStatus::Running),
            _ => None,
        },
        Some(Value::String(status)) => match status.as_str() {
            "completed" | "succeeded" | "success" => Some(SessionToolStatus::Succeeded),
            "failed" | "error" => Some(SessionToolStatus::Failed),
            "cancelled" | "canceled" => Some(SessionToolStatus::Cancelled),
            "running" | "in_progress" | "pending" | "preparing" | "prepared" | "started" => {
                Some(SessionToolStatus::Running)
            }
            _ => None,
        },
        _ => None,
    }
}

fn token_counts(value: Option<&Value>) -> Option<SessionTokenCounts> {
    let value = value?;
    let counts = SessionTokenCounts {
        input_tokens: usage::number(value, "input_tokens")
            .or_else(|| usage::number(value, "prompt_tokens")),
        output_tokens: usage::number(value, "output_tokens")
            .or_else(|| usage::number(value, "completion_tokens")),
        cache_read_tokens: usage::number(value, "cache_read_input_tokens")
            .or_else(|| usage::number(value, "cache_read_tokens")),
        cache_write_tokens: usage::number(value, "cache_creation_input_tokens")
            .or_else(|| usage::number(value, "cache_write_tokens")),
        reasoning_tokens: usage::number(value, "reasoning_tokens"),
    };
    (counts.input_tokens.is_some()
        || counts.output_tokens.is_some()
        || counts.cache_read_tokens.is_some()
        || counts.cache_write_tokens.is_some()
        || counts.reasoning_tokens.is_some())
    .then_some(counts)
}

fn exit_code(value: &Value) -> Option<i32> {
    value
        .pointer("/details/execution/exitCode")
        .or_else(|| value.pointer("/details/processOutput/exitCode"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn result_text(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for item in content {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        if !text.trim().is_empty() {
                            parts.push(text.to_owned());
                        }
                    }
                }
                Some("image") => parts.push("![图片](data:image/png;base64,[inline image])".to_string()),
                _ => {
                    if let Some(text) = item.as_str().filter(|text| !text.trim().is_empty()) {
                        parts.push(text.to_owned());
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        for key in ["stdout", "stderr"] {
            if let Some(text) = value
                .pointer(&format!("/details/processOutput/{key}"))
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
            {
                parts.push(text.to_owned());
            }
        }
    }
    if parts.is_empty() {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            return format!("[失败] {message}");
        }
        if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
            return format!("[失败] {}", json_text(error));
        }
    }
    parts.join("\n")
}

fn content_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .or_else(|| item.get("text").and_then(Value::as_str))
                })
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn parse_payload(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .or_else(|| Some(Value::String(text.clone()))),
        other => Some(other.clone()),
    }
}

fn mutation(
    id: String,
    role: CliSessionMessageRole,
    content: String,
    metadata: SessionMessageMetadata,
) -> SessionTranscriptMutation {
    SessionTranscriptMutation::Put {
        message: SessionTranscriptMessage {
            id,
            role,
            content,
            metadata,
        },
        priority: 1,
    }
}
