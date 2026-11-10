//! Crush message-row decoder.
//!
//! `parts` is a typed wrapper list: each part is `{type, data}` with text,
//! reasoning, tool-call, tool-result, shell-command, image and finish shapes.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionToolKind,
        SessionToolStatus,
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
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("crush-{sequence}"));
    let role = value.get("role").and_then(Value::as_str).unwrap_or_default();
    if role == "system" {
        return Vec::new();
    }
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        set_model(state, model);
    }
    let timestamp = value
        .get("created_at")
        .and_then(Value::as_i64)
        .and_then(|value| timestamp_from_unix(Some(value)));
    let base = SessionMessageMetadata {
        timestamp,
        model: state
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let role = match role {
        "user" => CliSessionMessageRole::User,
        "tool" => CliSessionMessageRole::Tool,
        _ => CliSessionMessageRole::Assistant,
    };
    let parts = value
        .get("parts")
        .and_then(|parts| {
            parts
                .as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .or_else(|| parts.as_array().map(|_| parts.clone()))
        })
        .and_then(|parts| parts.as_array().cloned())
        .unwrap_or_default();
    let mut result = Vec::new();
    let mut text = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
        let data = part.get("data").unwrap_or(&Value::Null);
        match kind {
            "text" => {
                if data.get("hidden").and_then(Value::as_bool) == Some(true)
                    || data
                        .get("context")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                {
                    if let Some(value) = data.get("text").and_then(Value::as_str) {
                        push(
                            &mut result,
                            format!("crush-{id}:context:{index:04}"),
                            CliSessionMessageRole::Assistant,
                            value.to_owned(),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Event,
                                injected: true,
                                ..base.clone()
                            },
                        );
                    }
                    continue;
                }
                if let Some(value) = data.get("text").and_then(Value::as_str) {
                    text.push(value.to_owned());
                }
            }
            "reasoning" => {
                if let Some(value) = data
                    .get("thinking")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    push(
                        &mut result,
                        format!("crush-{id}:thinking:{index:04}"),
                        CliSessionMessageRole::Assistant,
                        value.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                    );
                }
            }
            "tool_call" => {
                let call_id = data
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some(format!("crush-{id}-call-{index}")));
                let name = data.get("name").and_then(Value::as_str).unwrap_or("工具");
                let arguments = parse_arguments(data.get("input"));
                let mut metadata = presentation::tool_call(name, &arguments, call_id);
                metadata.timestamp = base.timestamp.clone();
                metadata.model = base.model.clone();
                metadata.group_id = base.group_id.clone();
                if data.get("finished").and_then(Value::as_bool) == Some(true) {
                    metadata.status = Some(SessionToolStatus::Succeeded);
                }
                push(
                    &mut result,
                    format!("crush-{id}:call:{index:04}"),
                    CliSessionMessageRole::Tool,
                    json_text(&arguments),
                    metadata,
                );
            }
            "tool_result" => {
                let call_id = data
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let content = data
                    .get("content")
                    .or_else(|| data.get("data"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let mut metadata = presentation::tool_result(&content, call_id.clone());
                metadata.timestamp = base.timestamp.clone();
                metadata.model = base.model.clone();
                metadata.group_id = base.group_id.clone();
                metadata.tool_kind = Some(SessionToolKind::Other);
                metadata.tool_name = data
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if data.get("is_error").and_then(Value::as_bool) == Some(true) {
                    metadata.status = Some(SessionToolStatus::Failed);
                } else if metadata.status.is_none() {
                    metadata.status = Some(SessionToolStatus::Succeeded);
                }
                push(
                    &mut result,
                    format!("crush-{id}:result:{index:04}"),
                    CliSessionMessageRole::Tool,
                    json_text(&content),
                    metadata,
                );
            }
            "shell_command" => {
                let call_id = format!("crush-{id}-shell-{index}");
                let command = data.get("command").cloned().unwrap_or(Value::Null);
                let output = data.get("output").cloned().unwrap_or(Value::Null);
                let mut call = presentation::tool_call("shell", &command, Some(call_id.clone()));
                call.timestamp = base.timestamp.clone();
                call.model = base.model.clone();
                call.group_id = base.group_id.clone();
                push(
                    &mut result,
                    format!("crush-{id}:shell:{index:04}"),
                    CliSessionMessageRole::Tool,
                    json_text(&command),
                    call,
                );
                let mut result_meta = presentation::tool_result(&output, Some(call_id));
                result_meta.timestamp = base.timestamp.clone();
                result_meta.model = base.model.clone();
                result_meta.group_id = base.group_id.clone();
                result_meta.tool_kind = Some(SessionToolKind::Shell);
                result_meta.exit_code = data
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .and_then(|code| i32::try_from(code).ok());
                result_meta.status = Some(if result_meta.exit_code == Some(0) {
                    SessionToolStatus::Succeeded
                } else {
                    SessionToolStatus::Failed
                });
                push(
                    &mut result,
                    format!("crush-{id}:shell-result:{index:04}"),
                    CliSessionMessageRole::Tool,
                    json_text(&output),
                    result_meta,
                );
            }
            "image_url" => {
                if let Some(url) = data.get("url").and_then(Value::as_str) {
                    push(
                        &mut result,
                        format!("crush-{id}:image:{index:04}"),
                        CliSessionMessageRole::User,
                        url.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Image,
                            ..base.clone()
                        },
                    );
                }
            }
            "binary" => {
                if let Some(reference) = binary_reference(data) {
                    push(
                        &mut result,
                        format!("crush-{id}:image:{index:04}"),
                        CliSessionMessageRole::User,
                        reference,
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Image,
                            ..base.clone()
                        },
                    );
                }
            }
            "finish" => {
                if let Some(message) = data
                    .get("message")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    push(
                        &mut result,
                        format!("crush-{id}:finish:{index:04}"),
                        CliSessionMessageRole::Assistant,
                        message.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Event,
                            ..base.clone()
                        },
                    );
                }
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        push(
            &mut result,
            format!("crush-{id}"),
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

fn binary_reference(data: &Value) -> Option<String> {
    data.get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            data.get("mime_type")
                .and_then(Value::as_str)
                .map(|mime| format!("data:{mime};base64,[inline image]"))
        })
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

fn set_model(state: &mut Value, model: &str) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    if let Some(object) = state.as_object_mut() {
        object.insert("model".into(), Value::String(model.to_owned()));
    }
}
