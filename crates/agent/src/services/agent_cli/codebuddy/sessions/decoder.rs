//! CodeBuddy JSONL transcript decoder.
//!
//! Records use the OpenAI Responses item vocabulary rather than Claude's
//! content blocks: `message` items carry `input_text`/`output_text` parts and
//! tool calls are separate `function_call` / `function_call_result` records.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolKind, SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, normalize_timestamp, presentation, usage},
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
        Some("function_call") => decode_call(&value, sequence, &mut result),
        Some("function_call_result") => decode_result(&value, sequence, &mut result),
        Some("reasoning") => decode_reasoning(&value, sequence, &mut result),
        _ => {}
    }
    result
}

fn decode_message(
    value: &Value,
    sequence: u64,
    state: &mut Value,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    if value
        .pointer("/providerData/skipRun")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return;
    }
    let id = record_id(value, sequence);
    let role = value.get("role").and_then(Value::as_str).unwrap_or_default();
    let timestamp = value.get("timestamp").and_then(record_time);
    if let Some(model) = value
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/providerData/model").and_then(Value::as_str))
    {
        set_model(state, model);
    }
    let model = state
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let base = SessionMessageMetadata {
        timestamp,
        model,
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let Some(blocks) = value.get("content").and_then(Value::as_array) else {
        return;
    };
    let mut text = Vec::new();
    let mut part = 0usize;
    for block in blocks {
        part += 1;
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or_default();
        match block_type {
            "input_text" | "output_text" | "text" => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push(value.to_owned());
                }
            }
            "thinking" | "reasoning_text" => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    push_text(
                        result,
                        format!("codebuddy-{id}:thinking:{part:06}"),
                        CliSessionMessageRole::Assistant,
                        value.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                        SessionMessageKind::Thinking,
                    );
                }
            }
            "image" | "input_image" => {
                if let Some(url) = block
                    .get("image_url")
                    .and_then(|value| {
                        value
                            .get("url")
                            .and_then(Value::as_str)
                            .or_else(|| value.as_str())
                    })
                    .or_else(|| block.get("url").and_then(Value::as_str))
                {
                    push_text(
                        result,
                        format!("codebuddy-{id}:image:{part:06}"),
                        CliSessionMessageRole::User,
                        url.to_owned(),
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
    let has_text = text.iter().any(|value| !value.trim().is_empty());
    if has_text {
        let role = match role {
            "user" => CliSessionMessageRole::User,
            // The system prompt is not a conversation row.
            "system" => return,
            _ => CliSessionMessageRole::Assistant,
        };
        push_text(
            result,
            format!("codebuddy-{id}"),
            role,
            text.join("\n"),
            base,
            SessionMessageKind::Text,
        );
    }
    if let Some(usage_value) = value.get("usage").or_else(|| value.pointer("/providerData/usage")) {
        push_usage(result, format!("codebuddy-usage-{id}"), usage_value, Some(format!("codebuddy-{id}")));
    }
}

fn decode_call(value: &Value, sequence: u64, result: &mut Vec<SessionTranscriptMutation>) {
    let id = record_id(value, sequence);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("工具");
    let call_id = value
        .get("callId")
        .or_else(|| value.get("call_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| Some(id.clone()));
    let arguments = parse_arguments(value.get("arguments").or_else(|| value.get("input")));
    let mut metadata = presentation::tool_call(name, &arguments, call_id);
    metadata.timestamp = value.get("timestamp").and_then(record_time);
    metadata.group_id = Some(id.clone());
    push_text(
        result,
        format!("codebuddy-{id}:call"),
        CliSessionMessageRole::Tool,
        json_text(&arguments),
        metadata,
        SessionMessageKind::ToolCall,
    );
}

fn decode_result(value: &Value, sequence: u64, result: &mut Vec<SessionTranscriptMutation>) {
    let id = record_id(value, sequence);
    let call_id = value
        .get("callId")
        .or_else(|| value.get("call_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let output = value.get("output").unwrap_or(&Value::Null);
    let mut metadata = presentation::tool_result(output, call_id.clone());
    metadata.timestamp = value.get("timestamp").and_then(record_time);
    metadata.group_id = Some(id.clone());
    metadata.tool_name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    metadata.exit_code = output
        .get("exitCode")
        .or_else(|| value.get("exitCode"))
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok());
    if let Some(status) = metadata.exit_code {
        metadata.status = Some(if status == 0 {
            SessionToolStatus::Succeeded
        } else {
            SessionToolStatus::Failed
        });
        metadata.tool_kind = Some(SessionToolKind::Shell);
    }
    let text = match output {
        Value::String(value) => value.clone(),
        Value::Object(_) => output
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| json_text(output)),
        Value::Null => String::new(),
        _ => json_text(output),
    };
    if !text.trim().is_empty() {
        push_text(
            result,
            format!("codebuddy-{id}:result"),
            CliSessionMessageRole::Tool,
            text,
            metadata,
            SessionMessageKind::ToolResult,
        );
    }
}

fn decode_reasoning(value: &Value, sequence: u64, result: &mut Vec<SessionTranscriptMutation>) {
    let id = record_id(value, sequence);
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value.get("summary").and_then(Value::as_array).map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        });
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        return;
    };
    push_text(
        result,
        format!("codebuddy-{id}:reasoning"),
        CliSessionMessageRole::Assistant,
        text,
        SessionMessageMetadata {
            kind: SessionMessageKind::Thinking,
            timestamp: value.get("timestamp").and_then(record_time),
            group_id: Some(id.clone()),
            ..Default::default()
        },
        SessionMessageKind::Thinking,
    );
}

fn push_usage(
    result: &mut Vec<SessionTranscriptMutation>,
    id: String,
    value: &Value,
    message_id: Option<String>,
) {
    result.push(usage::record(
        id,
        value.get("model").and_then(Value::as_str),
        SessionTokenCounts {
            input_tokens: number(value, &["input_tokens", "inputTokens", "prompt_tokens"]),
            output_tokens: number(value, &["output_tokens", "outputTokens", "completion_tokens"]),
            cache_read_tokens: number(
                value,
                &[
                    "cache_read_input_tokens",
                    "cacheReadInputTokens",
                    "cache_read_tokens",
                ],
            ),
            cache_write_tokens: number(
                value,
                &[
                    "cache_creation_input_tokens",
                    "cacheCreationInputTokens",
                    "cache_write_tokens",
                ],
            ),
            reasoning_tokens: number(value, &["reasoning_tokens", "reasoningOutputTokens"]),
        },
        false,
        false,
        message_id.as_deref(),
    ));
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

fn record_id(value: &Value, sequence: u64) -> String {
    value
        .get("uuid")
        .or_else(|| value.get("id"))
        .or_else(|| value.get("callId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("record-{sequence}"))
}

fn record_time(value: &Value) -> Option<String> {
    if let Some(number) = value.as_i64() {
        return crate::services::cli_sessions::timestamp_from_value(Some(number), false);
    }
    value
        .as_str()
        .and_then(|value| normalize_timestamp(Some(value)))
}

fn parse_arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone())),
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

fn number(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
}

fn set_model(state: &mut Value, model: &str) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    if let Some(object) = state.as_object_mut() {
        object.insert("model".into(), Value::String(model.to_owned()));
    }
}
