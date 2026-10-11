use super::*;
use crate::{
    models::{SessionMessageKind, SessionMessageMetadata},
    services::cli_sessions::presentation,
};

pub(super) fn decode(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionIndexMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let Some(payload) = value.get("payload") else {
        return Vec::new();
    };
    if !state.is_object() {
        *state = serde_json::json!({});
    }
    if value.get("type").and_then(Value::as_str) == Some("turn_context") {
        for key in ["model", "turn_id"] {
            if let Some(value) = payload.get(key).and_then(Value::as_str) {
                state[key] = Value::String(value.into());
            }
        }
        return Vec::new();
    }
    let mut metadata = SessionMessageMetadata {
        timestamp: normalize_timestamp(value.get("timestamp").and_then(Value::as_str)),
        model: state
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        turn_id: payload
            .get("turn_id")
            .or_else(|| state.get("turn_id"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        ..Default::default()
    };
    let mut messages = Vec::new();
    let kind = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let family = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if family == "event_msg" && kind == "task_started" {
        if let Some(turn) = payload.get("turn_id") {
            state["turn_id"] = turn.clone();
        }
    }
    if family == "event_msg" {
        if let Some(events) = native_tool_event(sequence, payload, &metadata) {
            return events;
        }
    }
    let (role, content, priority) = match (family, kind) {
        ("event_msg", "user_message" | "agent_message") => {
            let role = if kind == "user_message" {
                CliSessionMessageRole::User
            } else {
                CliSessionMessageRole::Assistant
            };
            let content = payload
                .get("message")
                .or_else(|| payload.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            (role, content, 1)
        }
        ("response_item", "message") => {
            let role = match payload.get("role").and_then(Value::as_str) {
                Some("user") => CliSessionMessageRole::User,
                Some("assistant") => CliSessionMessageRole::Assistant,
                Some("developer" | "system") => {
                    metadata.injected = true;
                    CliSessionMessageRole::Tool
                }
                _ => return Vec::new(),
            };
            if let Some(parts) = payload.get("content").and_then(Value::as_array) {
                for (part_index, part) in parts.iter().enumerate() {
                    if matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("reasoning" | "reasoning_text" | "summary_text")
                    ) {
                        if let Some(text) = part
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.trim().is_empty())
                        {
                            messages.push(put(
                                format!("codex-{sequence}-thinking-{part_index}"),
                                CliSessionMessageRole::Tool,
                                text.into(),
                                -1,
                                SessionMessageMetadata {
                                    kind: SessionMessageKind::Thinking,
                                    ..metadata.clone()
                                },
                            ));
                        }
                    }
                    if matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("input_image" | "image")
                    ) {
                        if let Some(image) = part
                            .get("image_url")
                            .or_else(|| part.get("url"))
                            .and_then(Value::as_str)
                        {
                            let image_metadata = SessionMessageMetadata {
                                kind: SessionMessageKind::Image,
                                ..metadata.clone()
                            };
                            messages.push(put(
                                format!("codex-{sequence}-image-{part_index}"),
                                role,
                                image.into(),
                                -1,
                                image_metadata,
                            ));
                        }
                    }
                }
            }
            let content = super::response_message_text(payload).unwrap_or_default();
            metadata.injected |= [
                "<environment_context>",
                "<system-reminder>",
                "# AGENTS.md instructions",
                "<INSTRUCTIONS>",
            ]
            .iter()
            .any(|prefix| content.trim_start().starts_with(prefix));
            (role, content, if metadata.injected { -1 } else { 0 })
        }
        ("response_item", "function_call" | "custom_tool_call") => {
            let input = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .unwrap_or(&Value::Null);
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("工具");
            let call = payload
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let native = presentation::tool_call(name, input, call.clone());
            metadata = SessionMessageMetadata {
                timestamp: metadata.timestamp,
                model: metadata.model,
                turn_id: metadata.turn_id,
                ..native
            };
            messages.push(put(
                call.map(|id| format!("codex-call-{id}"))
                    .unwrap_or_else(|| format!("codex-{sequence}")),
                CliSessionMessageRole::Tool,
                json_text(input),
                -1,
                metadata,
            ));
            return messages;
        }
        ("response_item", "function_call_output" | "custom_tool_call_output") => {
            let call = payload
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let output = payload.get("output").unwrap_or(&Value::Null);
            let native = presentation::tool_result(output, call.clone());
            metadata = SessionMessageMetadata {
                timestamp: metadata.timestamp,
                model: metadata.model,
                turn_id: metadata.turn_id,
                ..native
            };
            messages.push(put(
                call.map(|id| format!("codex-result-{id}"))
                    .unwrap_or_else(|| format!("codex-{sequence}")),
                CliSessionMessageRole::Tool,
                json_text(output),
                -1,
                metadata,
            ));
            return messages;
        }
        ("response_item", "reasoning") => {
            metadata.kind = SessionMessageKind::Thinking;
            let content = ["content", "summary"]
                .iter()
                .find_map(|field| {
                    let text = payload
                        .get(field)?
                        .as_array()?
                        .iter()
                        .filter_map(|item| item.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n");
                    (!text.trim().is_empty()).then_some(text)
                })
                .unwrap_or_default();
            (CliSessionMessageRole::Tool, content, -1)
        }
        ("event_msg", "agent_reasoning" | "agent_reasoning_raw_content") => {
            metadata.kind = SessionMessageKind::Thinking;
            (
                CliSessionMessageRole::Tool,
                payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                -1,
            )
        }
        ("event_msg", "task_complete" | "task_aborted" | "context_compacted") => {
            metadata.kind = SessionMessageKind::Event;
            let content = match kind {
                "task_aborted" => "本轮已中断",
                "context_compacted" => "上下文已压缩",
                _ => return Vec::new(),
            };
            (CliSessionMessageRole::Tool, content.into(), -1)
        }
        _ => return messages,
    };
    if !content.trim().is_empty() {
        messages.push(put(
            format!("codex-{sequence}"),
            role,
            content,
            priority,
            metadata,
        ));
    }
    messages
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

// Older rollouts store completed native operations in event_msg. Match by the
// official call_id so a parallel response_item can update, rather than duplicate,
// the same operation in the persisted index.
fn native_tool_event(
    sequence: u64,
    value: &Value,
    base: &SessionMessageMetadata,
) -> Option<Vec<SessionIndexMutation>> {
    let kind = value.get("type")?.as_str()?;
    let call = value
        .get("call_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("event-{sequence}"));
    let (name, input, output) = match kind {
        "mcp_tool_call_end" => {
            let invocation = value.get("invocation")?;
            let server = invocation
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let tool = invocation
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("工具");
            (
                format!("mcp__{server}__{tool}"),
                invocation.get("arguments").cloned(),
                value.get("result").cloned(),
            )
        }
        "exec_command_begin" => (
            "exec_command".into(),
            Some(serde_json::json!({"command": value.get("command"), "cwd": value.get("cwd")})),
            None,
        ),
        "exec_command_end" => ("exec_command".into(), None, Some(value.clone())),
        "patch_apply_end" => ("apply_patch".into(), None, Some(value.clone())),
        _ => return None,
    };
    let mut result = Vec::new();
    if let Some(input) = input {
        let metadata = presentation::tool_call(&name, &input, Some(call.clone()));
        result.push(put(
            format!("codex-call-{call}"),
            CliSessionMessageRole::Tool,
            json_text(&input),
            -1,
            SessionMessageMetadata {
                timestamp: base.timestamp.clone(),
                model: base.model.clone(),
                turn_id: base.turn_id.clone(),
                ..metadata
            },
        ));
    }
    if let Some(output) = output {
        let metadata = presentation::tool_result(&output, Some(call.clone()));
        let tool = presentation::tool_call(&name, &Value::Null, None);
        result.push(put(
            format!("codex-result-{call}"),
            CliSessionMessageRole::Tool,
            json_text(&output),
            -1,
            SessionMessageMetadata {
                timestamp: base.timestamp.clone(),
                model: base.model.clone(),
                turn_id: base.turn_id.clone(),
                tool_name: Some(name),
                tool_kind: tool.tool_kind,
                ..metadata
            },
        ));
    }
    Some(result)
}
