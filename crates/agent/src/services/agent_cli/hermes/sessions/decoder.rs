//! Hermes message-row decoder.
//!
//! One SQLite row is one model-visible message. Assistant rows may carry an
//! OpenAI-style `tool_calls` array; tool rows carry the result. The row id is
//! stable, so a database revision replay replaces rows instead of appending.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionToolKind,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, presentation, timestamp_from_unix},
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
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .unwrap_or_else(|| format!("hermes-{sequence}"));
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let display_kind = value
        .get("display_kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // Hidden rows are harness bookkeeping, and `session_meta` is a synthetic
    // summary row rather than a conversation turn.
    if display_kind == "hidden" || role == "session_meta" || role == "system" {
        return Vec::new();
    }
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_f64)
        .map(|value| value as i64)
        .and_then(|value| timestamp_from_unix(Some(value)));
    let model = state
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let base = SessionMessageMetadata {
        timestamp,
        model,
        group_id: Some(id.clone()),
        injected: display_kind == "internal_notification",
        ..Default::default()
    };
    let mut result = Vec::new();
    let role_kind = match role {
        "user" => CliSessionMessageRole::User,
        "tool" => CliSessionMessageRole::Tool,
        _ => CliSessionMessageRole::Assistant,
    };
    if let Some(reasoning) = value
        .get("reasoning")
        .or_else(|| value.get("reasoning_content"))
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        push(
            &mut result,
            format!("hermes-{id}:thinking"),
            CliSessionMessageRole::Assistant,
            reasoning.to_owned(),
            SessionMessageMetadata {
                kind: SessionMessageKind::Thinking,
                ..base.clone()
            },
        );
    }
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty());
    if let Some(content) = content {
        if role_kind == CliSessionMessageRole::Tool {
            let call_id = value
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let mut metadata = presentation::tool_result(&Value::String(content.to_owned()), call_id);
            metadata.timestamp = base.timestamp.clone();
            metadata.model = base.model.clone();
            metadata.group_id = base.group_id.clone();
            metadata.tool_kind = Some(
                tool_kind(
                    value
                        .get("tool_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
                .unwrap_or(SessionToolKind::Other),
            );
            if metadata.tool_name.is_none() {
                metadata.tool_name = value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            push(
                &mut result,
                format!("hermes-{id}"),
                CliSessionMessageRole::Tool,
                content.to_owned(),
                metadata,
            );
        } else {
            push(
                &mut result,
                format!("hermes-{id}"),
                role_kind,
                content.to_owned(),
                SessionMessageMetadata {
                    kind: if display_kind == "internal_notification" {
                        SessionMessageKind::Event
                    } else {
                        SessionMessageKind::Text
                    },
                    ..base.clone()
                },
            );
        }
    }
    if let Some(calls) = value.get("tool_calls") {
        let calls = calls
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .map(|value| value.to_owned())
            .or_else(|| calls.as_array().map(|_| calls.clone()))
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        for (index, call) in calls.iter().enumerate() {
            let name = call
                .pointer("/function/name")
                .or_else(|| call.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("工具");
            let call_id = call
                .get("call_id")
                .or_else(|| call.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(format!("hermes-{id}-call-{index}")));
            let arguments = call
                .pointer("/function/arguments")
                .or_else(|| call.get("arguments"))
                .cloned()
                .unwrap_or(Value::Null);
            let arguments = parse_arguments(&arguments);
            let mut metadata = presentation::tool_call(name, &arguments, call_id);
            metadata.timestamp = base.timestamp.clone();
            metadata.model = base.model.clone();
            metadata.group_id = base.group_id.clone();
            push(
                &mut result,
                format!("hermes-{id}:call:{index:04}"),
                CliSessionMessageRole::Tool,
                json_text(&arguments),
                metadata,
            );
        }
    }
    result
}

fn tool_kind(name: &str) -> Option<SessionToolKind> {
    if name.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    Some(if lower.contains("terminal") || lower.contains("shell") || lower.contains("exec") {
        SessionToolKind::Shell
    } else if lower.contains("read") {
        SessionToolKind::Read
    } else if lower.contains("write") {
        SessionToolKind::Write
    } else if lower.contains("edit") || lower.contains("patch") {
        SessionToolKind::Edit
    } else if lower.contains("search") || lower.contains("grep") {
        SessionToolKind::Search
    } else {
        SessionToolKind::Other
    })
}

fn parse_arguments(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        value => value.clone(),
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
