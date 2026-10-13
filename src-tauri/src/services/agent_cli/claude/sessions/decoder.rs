use super::*;
use crate::{
    models::{SessionMessageKind, SessionMessageMetadata, SessionToolStatus},
    services::cli_sessions::presentation,
};

pub(super) fn decode(
    path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionIndexMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    if !history::is_subagent_path(path)
        && value.get("isSidechain").and_then(Value::as_bool) == Some(true)
    {
        return Vec::new();
    }
    let role = match value.get("type").and_then(Value::as_str) {
        Some("user") => CliSessionMessageRole::User,
        Some("assistant") => CliSessionMessageRole::Assistant,
        _ => return Vec::new(),
    };
    let Some(message) = value.get("message") else {
        return Vec::new();
    };
    let Some(content) = message.get("content") else {
        return Vec::new();
    };
    let id = value
        .get("uuid")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{sequence}"));
    let parts = content.as_array().cloned().unwrap_or_else(|| {
        vec![serde_json::json!({"type":"text","text":content.as_str().unwrap_or_default()})]
    });
    let injected = value.get("isMeta").and_then(Value::as_bool) == Some(true);
    let is_question = role == CliSessionMessageRole::User
        && !injected
        && parts.iter().any(|part| {
            part.get("type").and_then(Value::as_str) == Some("image")
                || (part.get("type").and_then(Value::as_str) == Some("text")
                    && part
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(visible_user_text))
        });
    if !state.is_object() {
        *state = serde_json::json!({});
    }
    if is_question {
        state["turn"] = Value::String(id.clone());
    }
    let base = SessionMessageMetadata {
        timestamp: normalize_timestamp(value.get("timestamp").and_then(Value::as_str)),
        model: message
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        turn_id: state.get("turn").and_then(Value::as_str).map(str::to_owned),
        injected,
        ..Default::default()
    };
    let mut result = Vec::new();
    if role == CliSessionMessageRole::Assistant {
        if let Some(usage) = message.get("usage").filter(|value| value.is_object()) {
            use crate::services::cli_sessions::usage::{number, record};
            let read = number(usage, "cache_read_input_tokens");
            let write = number(usage, "cache_creation_input_tokens");
            let message_id = message
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| value.get("uuid").and_then(Value::as_str));
            let request_id = value
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let identity = serde_json::json!([message_id.unwrap_or(&id), request_id]).to_string();
            result.push(record(
                format!("claude-usage-{identity}"),
                base.model.as_deref(),
                crate::models::SessionTokenCounts {
                    input_tokens: number(usage, "input_tokens").map(|input| {
                        input
                            .saturating_add(read.unwrap_or_default())
                            .saturating_add(write.unwrap_or_default())
                    }),
                    output_tokens: number(usage, "output_tokens"),
                    cache_read_tokens: read,
                    cache_write_tokens: write,
                    reasoning_tokens: usage
                        .pointer("/output_tokens_details/reasoning_tokens")
                        .and_then(Value::as_u64),
                },
                false,
                message_id.is_none(),
                None,
            ));
        }
    }
    for (index, part) in parts.iter().enumerate() {
        let mut metadata = base.clone();
        let mut output_role = role;
        let mut priority = 1;
        let text = match part.get("type").and_then(Value::as_str).unwrap_or("text") {
            "text" => {
                let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                metadata.injected |=
                    role == CliSessionMessageRole::User && !visible_user_text(text);
                text.to_owned()
            }
            "thinking" => {
                metadata.kind = SessionMessageKind::Thinking;
                output_role = CliSessionMessageRole::Tool;
                priority = -1;
                part.get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into()
            }
            "tool_use" => {
                let input = part.get("input").unwrap_or(&Value::Null);
                let call = presentation::tool_call(
                    part.get("name").and_then(Value::as_str).unwrap_or("工具"),
                    input,
                    part.get("id").and_then(Value::as_str).map(str::to_owned),
                );
                metadata = SessionMessageMetadata {
                    timestamp: metadata.timestamp,
                    model: metadata.model,
                    turn_id: metadata.turn_id,
                    ..call
                };
                output_role = CliSessionMessageRole::Tool;
                priority = -1;
                crate::services::cli_sessions::json_text(input)
            }
            "tool_result" => {
                let native = presentation::tool_result(
                    part,
                    part.get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                );
                metadata = SessionMessageMetadata {
                    timestamp: metadata.timestamp,
                    model: metadata.model,
                    turn_id: metadata.turn_id,
                    status: native.status.or_else(|| {
                        Some(
                            if part.get("is_error").and_then(Value::as_bool) == Some(true) {
                                SessionToolStatus::Failed
                            } else {
                                SessionToolStatus::Succeeded
                            },
                        )
                    }),
                    ..native
                };
                output_role = CliSessionMessageRole::Tool;
                priority = -1;
                let content = part.get("content").unwrap_or(&Value::Null);
                if let Some(parts) = content.as_array() {
                    for (image_index, image) in parts.iter().enumerate().filter(|(_, part)| {
                        part.get("type").and_then(Value::as_str) == Some("image")
                    }) {
                        if let Some(content) = image_data(image) {
                            result.push(SessionIndexMutation::Put {
                                message: SessionIndexMessage {
                                    id: format!("claude-{id}-{index}-image-{image_index}"),
                                    role: CliSessionMessageRole::Tool,
                                    content,
                                    metadata: SessionMessageMetadata {
                                        kind: SessionMessageKind::Image,
                                        ..metadata.clone()
                                    },
                                },
                                priority: -1,
                            });
                        }
                    }
                    parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    crate::services::cli_sessions::json_text(content)
                }
            }
            "image" => {
                metadata.kind = SessionMessageKind::Image;
                priority = -1;
                image_data(part).unwrap_or_default()
            }
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        result.push(SessionIndexMutation::Put {
            message: SessionIndexMessage {
                id: format!("claude-{id}-{index}"),
                role: output_role,
                content: text,
                metadata,
            },
            priority,
        });
    }
    result
}

fn image_data(part: &Value) -> Option<String> {
    let source = part.get("source")?;
    if let (Some(media), Some(data)) = (
        source.get("media_type").and_then(Value::as_str),
        source.get("data").and_then(Value::as_str),
    ) {
        Some(format!("data:{media};base64,{data}"))
    } else {
        source.get("url").and_then(Value::as_str).map(str::to_owned)
    }
}
