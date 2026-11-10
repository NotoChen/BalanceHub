//! AtomCode event-log decoder.
//!
//! Each line of `<session>.events` is `{"seq":…,"at":…,"event":{"kind":…}}`.
//! Only conversation-bearing events become transcript rows; checkpoints,
//! request headers, usage counters and titles are metadata.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, presentation},
    },
};
use serde_json::Value;

pub(super) fn decode(
    _path: &std::path::Path,
    sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let Some(event) = value.get("event") else {
        return Vec::new();
    };
    let kind = event.get("kind").and_then(Value::as_str).unwrap_or_default();
    let id = format!("atomcode-{sequence}");
    let mut base = SessionMessageMetadata {
        group_id: Some(id.clone()),
        timestamp: value
            .get("at")
            .and_then(Value::as_i64)
            .and_then(|at| crate::services::cli_sessions::timestamp_from_value(Some(at), true)),
        turn_id: event
            .get("turn")
            .and_then(Value::as_i64)
            .map(|turn| turn.to_string()),
        ..Default::default()
    };
    let mut result = Vec::new();
    match kind {
        "user_message" => {
            if let Some(text) = event.get("text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    push_text(
                        &mut result,
                        format!("{id}:user"),
                        CliSessionMessageRole::User,
                        text.to_owned(),
                        base,
                        SessionMessageKind::Text,
                    );
                }
            }
        }
        "assistant_message" => {
            if let Some(text) = event.get("text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    let mut metadata = base.clone();
                    metadata.model = event
                        .pointer("/meta/provider_model")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    push_text(
                        &mut result,
                        format!("{id}:text"),
                        CliSessionMessageRole::Assistant,
                        text.to_owned(),
                        metadata,
                        SessionMessageKind::Text,
                    );
                }
            }
            if let Some(reasoning) = event.get("reasoning").and_then(Value::as_str) {
                if !reasoning.trim().is_empty() {
                    push_text(
                        &mut result,
                        format!("{id}:reasoning"),
                        CliSessionMessageRole::Assistant,
                        reasoning.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                        SessionMessageKind::Thinking,
                    );
                }
            }
            if let Some(calls) = event.get("tool_calls").and_then(Value::as_array) {
                for (index, call) in calls.iter().enumerate() {
                    push_tool_call(&mut result, &id, index, call, &base);
                }
            }
        }
        "tool_result_logged" => {
            let call_id = event
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let content = event.get("content").and_then(Value::as_str).unwrap_or_default();
            let mut metadata = presentation::tool_result(
                &serde_json::json!({ "content": content }),
                call_id,
            );
            metadata.group_id = Some(id.clone());
            metadata.status = Some(
                if event
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    SessionToolStatus::Failed
                } else {
                    SessionToolStatus::Succeeded
                },
            );
            push_text(
                &mut result,
                format!("{id}:result"),
                CliSessionMessageRole::Tool,
                content.to_owned(),
                metadata,
                SessionMessageKind::ToolResult,
            );
        }
        "turn_end" => {
            if let Some(error) = event.get("error").and_then(Value::as_str) {
                if !error.trim().is_empty() {
                    base.title = event
                        .get("stop")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    push_text(
                        &mut result,
                        format!("{id}:error"),
                        CliSessionMessageRole::Assistant,
                        error.to_owned(),
                        base,
                        SessionMessageKind::Event,
                    );
                }
            }
        }
        _ => {}
    }
    result
}

fn push_tool_call(
    result: &mut Vec<SessionTranscriptMutation>,
    id: &str,
    index: usize,
    call: &Value,
    base: &SessionMessageMetadata,
) {
    let name = call.get("name").and_then(Value::as_str).unwrap_or("工具");
    let call_id = call
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| Some(format!("{id}:call:{index}")));
    let arguments = parse_arguments(call.get("arguments"));
    let mut metadata = presentation::tool_call(name, &arguments, call_id);
    metadata.group_id = Some(id.to_string());
    metadata.turn_id = base.turn_id.clone();
    metadata.timestamp = base.timestamp.clone();
    push_text(
        result,
        format!("{id}:call:{index:06}"),
        CliSessionMessageRole::Tool,
        json_text(&arguments),
        metadata,
        SessionMessageKind::ToolCall,
    );
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
