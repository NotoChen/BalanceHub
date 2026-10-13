use super::*;
use crate::{
    models::{SessionMessageKind, SessionMessageMetadata, SessionToolStatus},
    services::cli_sessions::presentation,
};

pub(super) fn update(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionIndexMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let Some(update) = value.get("params").and_then(|params| params.get("update")) else {
        return Vec::new();
    };
    if !state.is_object() {
        *state = serde_json::json!({});
    }
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let model = update
        .get("_meta")
        .and_then(|meta| meta.get("modelId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let timestamp = timestamp_from_unix(value.get("timestamp").and_then(Value::as_i64));
    let base = SessionMessageMetadata {
        timestamp,
        model,
        turn_id: state.get("turn").and_then(Value::as_str).map(str::to_owned),
        ..Default::default()
    };
    if kind == "turn_completed" {
        if let Some(usage) = update.get("usage") {
            use crate::services::cli_sessions::usage::{number, record};
            let turn = base.turn_id.clone().unwrap_or_else(|| sequence.to_string());
            let values = usage.get("modelUsage").and_then(Value::as_object);
            let entries: Vec<(Option<&str>, &Value)> = values
                .filter(|values| !values.is_empty())
                .map(|values| {
                    values
                        .iter()
                        .map(|(model, value)| (Some(model.as_str()), value))
                        .collect()
                })
                .unwrap_or_else(|| vec![(base.model.as_deref(), usage)]);
            return entries
                .into_iter()
                .map(|(model, usage)| {
                    record(
                        format!("grok-usage-{turn}-{}", model.unwrap_or_default()),
                        model,
                        crate::models::SessionTokenCounts {
                            input_tokens: number(usage, "inputTokens"),
                            output_tokens: number(usage, "outputTokens"),
                            cache_read_tokens: number(usage, "cachedReadTokens"),
                            cache_write_tokens: number(usage, "cacheCreationTokens"),
                            reasoning_tokens: None,
                        },
                        false,
                        base.turn_id.is_none(),
                        None,
                    )
                })
                .collect();
        }
    }
    if kind.contains("tool") {
        state["role"] = Value::Null;
        let call = update
            .get("toolCallId")
            .or_else(|| update.get("tool_call_id"))
            .or_else(|| update.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{sequence}"));
        let name = update
            .get("toolName")
            .or_else(|| update.get("name"))
            .or_else(|| update.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("工具");
        let mut result = Vec::new();
        if kind == "tool_call" || update.get("rawInput").is_some() {
            let input = update
                .get("rawInput")
                .or_else(|| update.get("input"))
                .unwrap_or(&Value::Null);
            let native = presentation::tool_call(name, input, Some(call.clone()));
            result.push(put(
                format!("grok-call-{call}"),
                CliSessionMessageRole::Tool,
                crate::services::cli_sessions::json_text(input),
                -1,
                SessionMessageMetadata {
                    timestamp: base.timestamp.clone(),
                    model: base.model.clone(),
                    turn_id: base.turn_id.clone(),
                    ..native
                },
            ));
        }
        if let Some(output) = update
            .get("rawOutput")
            .or_else(|| update.get("content"))
            .or_else(|| update.get("title"))
        {
            let mut native = presentation::tool_result(update, Some(call.clone()));
            if native.status.is_none() {
                native.status = match kind {
                    "tool_call_completed" => Some(SessionToolStatus::Succeeded),
                    "tool_call_failed" => Some(SessionToolStatus::Failed),
                    "tool_call_pending" => Some(SessionToolStatus::Running),
                    _ => None,
                };
            }
            native.tool_kind = presentation::tool_call(name, &Value::Null, None).tool_kind;
            result.push(put(
                format!("grok-result-{call}"),
                CliSessionMessageRole::Tool,
                crate::services::cli_sessions::json_text(output),
                -1,
                SessionMessageMetadata {
                    timestamp: base.timestamp,
                    model: base.model,
                    turn_id: base.turn_id,
                    tool_name: Some(name.into()),
                    ..native
                },
            ));
        }
        return result;
    }
    let (role, kind_key, message_kind) = match kind {
        "user_message_chunk" => (
            CliSessionMessageRole::User,
            "user",
            SessionMessageKind::Text,
        ),
        "agent_message_chunk" => (
            CliSessionMessageRole::Assistant,
            "assistant",
            SessionMessageKind::Text,
        ),
        "agent_thought_chunk" => (
            CliSessionMessageRole::Tool,
            "thinking",
            SessionMessageKind::Thinking,
        ),
        _ => return Vec::new(),
    };
    let Some(content) = update.get("content").and_then(content_text) else {
        return Vec::new();
    };
    let append = state.get("role").and_then(Value::as_str) == Some(kind_key);
    let id = if append {
        state.get("id").and_then(Value::as_str).map(str::to_owned)
    } else {
        None
    }
    .unwrap_or_else(|| format!("grok-{sequence}"));
    if role == CliSessionMessageRole::User && !append {
        state["turn"] = Value::String(id.clone());
    }
    state["role"] = Value::String(kind_key.into());
    state["id"] = Value::String(id.clone());
    let metadata = SessionMessageMetadata {
        kind: message_kind,
        turn_id: state.get("turn").and_then(Value::as_str).map(str::to_owned),
        ..base
    };
    let priority = if role == CliSessionMessageRole::Tool {
        -1
    } else {
        1
    };
    let message = SessionIndexMessage {
        id,
        role,
        content,
        metadata,
    };
    vec![if append {
        SessionIndexMutation::Append { message, priority }
    } else {
        SessionIndexMutation::Put { message, priority }
    }]
}

pub(super) fn chat(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionIndexMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let native_kind = value.get("type").and_then(Value::as_str);
    let role = match native_kind {
        Some("user") => CliSessionMessageRole::User,
        Some("assistant") => CliSessionMessageRole::Assistant,
        Some("thought" | "reasoning") => CliSessionMessageRole::Tool,
        _ => return Vec::new(),
    };
    if !state.is_object() {
        *state = serde_json::json!({});
    }
    if role == CliSessionMessageRole::User {
        state["turn"] = Value::String(format!("grok-chat-{sequence}"));
    }
    let base = SessionMessageMetadata {
        timestamp: normalize_timestamp(
            value
                .get("timestamp")
                .or_else(|| value.get("ts"))
                .and_then(Value::as_str),
        ),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        turn_id: state.get("turn").and_then(Value::as_str).map(str::to_owned),
        ..Default::default()
    };
    let mut result = Vec::new();
    let content = value
        .get("content")
        .or_else(|| value.get("data"))
        .unwrap_or(&Value::Null);
    if let Some(text) = content.as_str() {
        if !text.is_empty() {
            result.push(put(
                format!("grok-{sequence}"),
                role,
                text.into(),
                if role == CliSessionMessageRole::Tool {
                    -2
                } else {
                    0
                },
                SessionMessageMetadata {
                    kind: if role == CliSessionMessageRole::Tool {
                        SessionMessageKind::Thinking
                    } else {
                        SessionMessageKind::Text
                    },
                    ..base.clone()
                },
            ));
        }
    }
    if let Some(parts) = content.as_array() {
        for (index, part) in parts.iter().enumerate() {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
            let id = format!("grok-{sequence}-{index}");
            if matches!(kind, "text" | "reasoning" | "thinking") {
                let text = part
                    .get("text")
                    .or_else(|| part.get("thinking"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !text.is_empty() {
                    let thinking = kind != "text";
                    result.push(put(
                        id,
                        if thinking {
                            CliSessionMessageRole::Tool
                        } else {
                            role
                        },
                        text.into(),
                        if thinking { -2 } else { 0 },
                        SessionMessageMetadata {
                            kind: if thinking {
                                SessionMessageKind::Thinking
                            } else {
                                SessionMessageKind::Text
                            },
                            ..base.clone()
                        },
                    ));
                }
            } else if kind == "tool_use" || kind == "tool_call" {
                let input = part
                    .get("input")
                    .or_else(|| part.get("arguments"))
                    .or_else(|| part.get("args"))
                    .unwrap_or(&Value::Null);
                let call_id = Some(
                    part.get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| id.clone()),
                );
                let native = presentation::tool_call(
                    part.get("name").and_then(Value::as_str).unwrap_or("工具"),
                    input,
                    call_id.clone(),
                );
                result.push(put(
                    id.clone(),
                    CliSessionMessageRole::Tool,
                    crate::services::cli_sessions::json_text(input),
                    -2,
                    SessionMessageMetadata {
                        timestamp: base.timestamp.clone(),
                        model: base.model.clone(),
                        turn_id: base.turn_id.clone(),
                        ..native
                    },
                ));
                if let Some(output) = part.get("result").or_else(|| part.get("output")) {
                    let native = presentation::tool_result(output, call_id);
                    result.push(put(
                        format!("{id}-result"),
                        CliSessionMessageRole::Tool,
                        crate::services::cli_sessions::json_text(output),
                        -2,
                        SessionMessageMetadata {
                            timestamp: base.timestamp.clone(),
                            model: base.model.clone(),
                            turn_id: base.turn_id.clone(),
                            ..native
                        },
                    ));
                }
            } else if kind == "tool_result" {
                let native = presentation::tool_result(
                    part,
                    part.get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                );
                result.push(put(
                    id,
                    CliSessionMessageRole::Tool,
                    crate::services::cli_sessions::json_text(
                        part.get("content").unwrap_or(&Value::Null),
                    ),
                    -2,
                    SessionMessageMetadata {
                        timestamp: base.timestamp.clone(),
                        model: base.model.clone(),
                        turn_id: base.turn_id.clone(),
                        ..native
                    },
                ));
            } else if matches!(kind, "image" | "image_url") {
                let image = part
                    .get("image_url")
                    .and_then(|image| {
                        image
                            .as_str()
                            .or_else(|| image.get("url").and_then(Value::as_str))
                    })
                    .or_else(|| part.get("url").and_then(Value::as_str));
                if let Some(image) = image {
                    result.push(put(
                        id,
                        role,
                        image.into(),
                        -2,
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Image,
                            ..base.clone()
                        },
                    ));
                }
            }
        }
    }
    result
}

fn put(
    id: String,
    role: CliSessionMessageRole,
    content: String,
    priority: i64,
    metadata: SessionMessageMetadata,
) -> SessionIndexMutation {
    SessionIndexMutation::Put {
        message: SessionIndexMessage {
            id,
            role,
            content,
            metadata,
        },
        priority,
    }
}
