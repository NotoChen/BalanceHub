use super::*;
use crate::{
    models::{SessionMessageKind, SessionMessageMetadata},
    services::cli_sessions::presentation,
};

pub(super) fn decode(
    _path: &Path,
    _sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionIndexMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    if let Some(id) = value.get("$rewindTo").and_then(Value::as_str) {
        return vec![SessionIndexMutation::Rewind(format!("gemini-{id}"))];
    }
    let mut result = Vec::new();
    if let Some(messages) = value
        .get("$set")
        .and_then(|value| value.get("messages"))
        .or_else(|| value.get("messages"))
        .and_then(Value::as_array)
    {
        result.push(SessionIndexMutation::Clear);
        for message in messages {
            decode_message(message, &mut result);
        }
    } else if value.get("id").is_some() {
        decode_message(&value, &mut result);
    }
    result
}

fn decode_message(value: &Value, result: &mut Vec<SessionIndexMutation>) {
    let Some(message) = MessageSummary::from_value(value) else {
        return;
    };
    let id = format!("gemini-{}", message.id);
    result.push(SessionIndexMutation::RemoveGroup(id.clone()));
    let role = match message.kind.as_str() {
        "user" => CliSessionMessageRole::User,
        "gemini" => CliSessionMessageRole::Assistant,
        _ => return,
    };
    let base = SessionMessageMetadata {
        timestamp: message.timestamp,
        model: message.model,
        group_id: Some(id.clone()),
        turn_id: (role == CliSessionMessageRole::User).then(|| id.clone()),
        ..Default::default()
    };
    if role == CliSessionMessageRole::Assistant {
        if let Some(usage) = value.get("tokens").filter(|value| value.is_object()) {
            use crate::services::cli_sessions::usage::{number, record};
            let thoughts = number(usage, "thoughts");
            result.push(record(
                format!("gemini-usage-{}", message.id),
                base.model.as_deref(),
                crate::models::SessionTokenCounts {
                    input_tokens: number(usage, "input"),
                    output_tokens: number(usage, "output")
                        .map(|output| output.saturating_add(thoughts.unwrap_or_default())),
                    cache_read_tokens: number(usage, "cached"),
                    cache_write_tokens: None,
                    reasoning_tokens: thoughts,
                },
                false,
                false,
                Some(&id),
            ));
        }
    }
    if let Some(content) = message.text.filter(|content| !content.trim().is_empty()) {
        let metadata = SessionMessageMetadata {
            injected: role == CliSessionMessageRole::User
                && is_ignored_user_content(content.trim()),
            ..base.clone()
        };
        result.push(put(id.clone(), role, content, metadata));
    }
    let mut part = 0;
    let mut add = |content: String, metadata: SessionMessageMetadata| {
        part += 1;
        if !content.trim().is_empty() {
            result.push(put(
                format!("{id}:{part:06}"),
                if metadata.kind == SessionMessageKind::Image {
                    role
                } else {
                    CliSessionMessageRole::Tool
                },
                content,
                metadata,
            ));
        }
    };
    if let Some(thoughts) = value.get("thoughts").and_then(Value::as_array) {
        for thought in thoughts {
            let text = ["subject", "description", "text"]
                .iter()
                .filter_map(|field| thought.get(field).and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            add(
                text,
                SessionMessageMetadata {
                    kind: SessionMessageKind::Thinking,
                    ..base.clone()
                },
            );
        }
    }
    if let Some(tools) = value.get("toolCalls").and_then(Value::as_array) {
        for (index, tool) in tools.iter().enumerate() {
            let call = Some(
                tool.get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{id}-tool-{index}")),
            );
            let input = tool
                .get("args")
                .or_else(|| tool.get("arguments"))
                .unwrap_or(&Value::Null);
            let native = presentation::tool_call(
                tool.get("name").and_then(Value::as_str).unwrap_or("工具"),
                input,
                call.clone(),
            );
            add(
                crate::services::cli_sessions::json_text(input),
                SessionMessageMetadata {
                    timestamp: base.timestamp.clone(),
                    model: base.model.clone(),
                    group_id: base.group_id.clone(),
                    ..native
                },
            );
            if let Some(output) = tool.get("result").or_else(|| tool.get("response")) {
                let native = presentation::tool_result(tool, call);
                add(
                    crate::services::cli_sessions::json_text(output),
                    SessionMessageMetadata {
                        timestamp: base.timestamp.clone(),
                        model: base.model.clone(),
                        group_id: base.group_id.clone(),
                        ..native
                    },
                );
            }
        }
    }
    if let Some(parts) = value.get("content").and_then(Value::as_array) {
        for part in parts {
            if part.get("thought").and_then(Value::as_bool) == Some(true)
                || matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("thinking" | "reasoning")
                )
            {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    add(
                        text.into(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                    );
                }
            }
            if let Some(data) = part.get("inlineData").or_else(|| part.get("inline_data")) {
                if let (Some(media), Some(bytes)) = (
                    data.get("mimeType")
                        .or_else(|| data.get("mime_type"))
                        .and_then(Value::as_str),
                    data.get("data").and_then(Value::as_str),
                ) {
                    add(
                        format!("data:{media};base64,{bytes}"),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Image,
                            ..base.clone()
                        },
                    );
                }
            }
        }
    }
}

fn put(
    id: String,
    role: CliSessionMessageRole,
    content: String,
    metadata: SessionMessageMetadata,
) -> SessionIndexMutation {
    SessionIndexMutation::Put {
        message: SessionIndexMessage {
            id,
            role,
            content,
            metadata,
        },
        priority: 1,
    }
}
