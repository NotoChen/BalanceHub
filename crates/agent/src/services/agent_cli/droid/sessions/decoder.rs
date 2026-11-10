//! Factory Droid JSONL transcript decoder.
//!
//! Every record is one of `session_start`, `message` or a state snapshot such
//! as `compaction_state`. Message content uses Anthropic block shapes
//! (`text`, `thinking`, `tool_use`, `tool_result`). Records marked
//! `visibility: "llm_only"` are hidden from the user and skipped here.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{
            json_text, normalize_timestamp, presentation, timestamp_from_value,
            usage::{number, record},
        },
    },
};
use serde_json::Value;
use std::{
    fs,
    path::Path,
};

const MAX_SETTINGS_BYTES: u64 = 512 * 1024;

pub(super) fn decode(
    path: &Path,
    sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let mut result = Vec::new();
    if !state
        .get("droidUsage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if !state.is_object() {
            *state = serde_json::json!({});
        }
        state["droidUsage"] = Value::Bool(true);
        if let Some(usage) = settings_usage(path) {
            result.push(usage);
        }
    }
    let Ok(text) = std::str::from_utf8(line) else {
        return result;
    };
    let Some(value) = serde_json::from_str::<Value>(text.trim()).ok() else {
        return result;
    };
    result.extend(match value.get("type").and_then(Value::as_str) {
        Some("message") => decode_message(&value, sequence),
        Some("compaction_state") => decode_compaction(&value, sequence),
        _ => Vec::new(),
    });
    result
}

/// Token totals live in the sibling `<session-id>.settings.json`, which the
/// transcript reader cannot see; load it once per decoded source.
fn settings_usage(path: &Path) -> Option<SessionTranscriptMutation> {
    let settings_path = path.with_extension("settings.json");
    let metadata = fs::metadata(&settings_path).ok()?;
    if metadata.len() > MAX_SETTINGS_BYTES {
        return None;
    }
    let text = fs::read_to_string(&settings_path).ok()?;
    let settings = serde_json::from_str::<Value>(&text).ok()?;
    let usage = settings.get("tokenUsage")?;
    let input = number(usage, "inputTokens");
    let output = number(usage, "outputTokens");
    let read = number(usage, "cacheReadTokens");
    let write = number(usage, "cacheCreationTokens");
    if input.is_none() && output.is_none() && read.is_none() && write.is_none() {
        return None;
    }
    let model = settings
        .get("model")
        .or_else(|| settings.get("modelId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let id = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(record(
        format!("droid-usage-{id}"),
        model,
        SessionTokenCounts {
            input_tokens: input.map(|value| {
                value
                    .saturating_add(read.unwrap_or_default())
                    .saturating_add(write.unwrap_or_default())
            }),
            output_tokens: output,
            cache_read_tokens: read,
            cache_write_tokens: write,
            reasoning_tokens: number(usage, "thinkingTokens"),
        },
        true,
        false,
        None,
    ))
}

fn decode_message(value: &Value, sequence: u64) -> Vec<SessionTranscriptMutation> {
    let message = value.get("message").unwrap_or(&Value::Null);
    if message
        .get("visibility")
        .and_then(Value::as_str)
        .is_some_and(|visibility| visibility == "llm_only")
    {
        return Vec::new();
    }
    let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
    if !matches!(role, "user" | "assistant") {
        return Vec::new();
    }
    let message_id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("line-{sequence}"));
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)))
        .or_else(|| {
            timestamp_from_value(
                message
                    .get("createdAt")
                    .or_else(|| value.get("createdAt"))
                    .and_then(Value::as_i64),
                true,
            )
        });
    let model = message
        .get("modelId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let mut result = Vec::new();
    let blocks = message
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, block) in blocks.iter().enumerate() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or_default();
        let metadata_base = |kind, turn: &str| SessionMessageMetadata {
            kind,
            timestamp: timestamp.clone(),
            model: model.clone(),
            turn_id: Some(turn.to_owned()),
            group_id: Some(message_id.clone()),
            ..Default::default()
        };
        match block_type {
            "text" => {
                let Some(text) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    continue;
                };
                result.push(mutation(
                    format!("droid-{message_id}:{index}"),
                    if role == "user" {
                        CliSessionMessageRole::User
                    } else {
                        CliSessionMessageRole::Assistant
                    },
                    text.to_owned(),
                    metadata_base(
                        SessionMessageKind::Text,
                        message_id.as_str(),
                    ),
                ));
            }
            "thinking" => {
                let text = block
                    .get("thinking")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty());
                let Some(text) = text else {
                    continue;
                };
                result.push(mutation(
                    format!("droid-{message_id}:{index}"),
                    CliSessionMessageRole::Assistant,
                    text.to_owned(),
                    metadata_base(
                        SessionMessageKind::Thinking,
                        message_id.as_str(),
                    ),
                ));
            }
            "tool_use" => {
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{message_id}-{index}"));
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("工具");
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                let mut metadata = presentation::tool_call(name, &input, Some(call_id.clone()));
                metadata.timestamp = timestamp.clone();
                metadata.model = model.clone();
                metadata.turn_id = Some(message_id.clone());
                metadata.group_id = Some(message_id.clone());
                metadata.status = Some(SessionToolStatus::Running);
                result.push(mutation(
                    format!("droid-call-{call_id}"),
                    CliSessionMessageRole::Tool,
                    json_text(&input),
                    metadata,
                ));
            }
            "image" => {
                let Some(content) = image_data(block) else {
                    continue;
                };
                result.push(mutation(
                    format!("droid-{message_id}:{index}"),
                    CliSessionMessageRole::Tool,
                    content,
                    metadata_base(SessionMessageKind::Image, message_id.as_str()),
                ));
            }
            "tool_result" => {
                let call_id = block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{message_id}-{index}"));
                let is_error = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut metadata =
                    presentation::tool_result(block, Some(call_id.clone()));
                metadata.timestamp = timestamp.clone();
                metadata.model = model.clone();
                metadata.turn_id = Some(message_id.clone());
                metadata.group_id = Some(message_id.clone());
                metadata.call_id = Some(call_id.clone());
                if is_error {
                    metadata.status = Some(SessionToolStatus::Failed);
                } else if metadata.status.is_none() {
                    metadata.status = Some(SessionToolStatus::Succeeded);
                }
                let content = block_content(block.get("content")).unwrap_or_else(|| {
                    if is_error {
                        "工具执行失败".to_string()
                    } else {
                        "工具已执行完成".to_string()
                    }
                });
                result.push(mutation(
                    format!("droid-result-{call_id}"),
                    CliSessionMessageRole::Tool,
                    content,
                    metadata,
                ));
            }
            _ => {}
        }
    }
    result
}

fn decode_compaction(value: &Value, sequence: u64) -> Vec<SessionTranscriptMutation> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("line-{sequence}"));
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)));
    let removed = value
        .get("removedCount")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .or_else(|| {
            value
                .get("removedMessageIds")
                .and_then(Value::as_array)
                .map(Vec::len)
        })
        .unwrap_or_default();
    let content = value
        .get("summaryText")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if removed > 0 {
                format!("上下文已压缩，移除 {removed} 条消息")
            } else {
                "上下文已压缩".to_string()
            }
        });
    vec![mutation(
        format!("droid-compaction-{id}"),
        CliSessionMessageRole::Assistant,
        content,
        SessionMessageMetadata {
            kind: SessionMessageKind::Event,
            timestamp,
            group_id: Some(id),
            ..Default::default()
        },
    )]
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

fn block_content(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if text.trim().is_empty() => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let parts = blocks
                .iter()
                .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                    Some("text") => block.get("text").and_then(Value::as_str),
                    Some("image") => Some("[图片]"),
                    _ => None,
                })
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        other => Some(json_text(other)),
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
