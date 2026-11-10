//! MothX entry decoder.
//!
//! Entries are an append-only log: message entries carry provider messages,
//! model-change entries update decoder state, and compaction/branch entries
//! become visible events. The entry id is unique across the shared database,
//! so replaying a revision replaces the same rows.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{clean_text, json_text, normalize_timestamp, presentation, usage},
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
    let Ok(entry) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let entry_type = entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = entry
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("mothx-{sequence}"));
    let timestamp = entry
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)));
    let model = state
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let base = SessionMessageMetadata {
        timestamp,
        model: model.clone(),
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let mut result = Vec::new();
    match entry_type {
        "model_change" => {
            if let Some(model) = entry
                .get("modelId")
                .or_else(|| entry.get("model"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                set_state(state, "model", model);
            }
        }
        "message" => {
            let Some(message) = entry.get("message") else {
                return Vec::new();
            };
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let injected = message.get("systemInjected").and_then(Value::as_bool) == Some(true);
            if role == "assistant" {
                if let Some(record) = usage_from_message(&id, model.as_deref(), message) {
                    result.push(record);
                }
            }
            if role == "toolResult" {
                emit_tool_result(&id, message, &base, &mut result);
            } else {
                emit_message(&id, role, injected, message, &base, &mut result);
            }
        }
        "content_override" => {
            if let Some(message) = entry.get("message") {
                let role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if role == "toolResult" {
                    emit_tool_result(&id, message, &base, &mut result);
                } else {
                    emit_message(&id, role, false, message, &base, &mut result);
                }
            }
        }
        "compaction" | "branch_summary" => {
            let summary = entry
                .get("summary")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            let text = match (entry_type, summary) {
                ("compaction", Some(summary)) => {
                    format!("[上下文压缩]\n{}", clean_text(summary, 2_000).unwrap_or_default())
                }
                ("compaction", None) => "[上下文压缩]".into(),
                (_, Some(summary)) => {
                    format!("[分支摘要]\n{}", clean_text(summary, 2_000).unwrap_or_default())
                }
                (_, None) => "[分支摘要]".into(),
            };
            push(
                &mut result,
                id,
                CliSessionMessageRole::Assistant,
                text,
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    injected: true,
                    ..base
                },
            );
        }
        "custom_message" | "custom" => {
            let text = entry
                .get("text")
                .or_else(|| entry.get("content"))
                .or_else(|| entry.get("message").and_then(|message| message.get("content")))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            if let Some(text) = text {
                push(
                    &mut result,
                    id,
                    CliSessionMessageRole::Assistant,
                    text.to_owned(),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Event,
                        injected: true,
                        ..base
                    },
                );
            }
        }
        _ => {}
    }
    result
}

fn emit_message(
    id: &str,
    role: &str,
    injected: bool,
    message: &Value,
    base: &SessionMessageMetadata,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let role_kind = if role == "user" {
        CliSessionMessageRole::User
    } else {
        CliSessionMessageRole::Assistant
    };
    if let Some(content) = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        push(
            result,
            format!("{id}:text"),
            role_kind,
            content.to_owned(),
            SessionMessageMetadata {
                kind: if injected {
                    SessionMessageKind::Event
                } else {
                    SessionMessageKind::Text
                },
                injected,
                ..base.clone()
            },
        );
    }
    let mut index = 0usize;
    if let Some(contents) = message.get("contents").and_then(Value::as_array) {
        for block in contents {
            let block_type = block.get("type").and_then(Value::as_str).unwrap_or_default();
            let block_id = format!("{id}:block:{index:04}");
            index += 1;
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        push(
                            result,
                            block_id,
                            role_kind,
                            text.to_owned(),
                            SessionMessageMetadata {
                                kind: if injected {
                                    SessionMessageKind::Event
                                } else {
                                    SessionMessageKind::Text
                                },
                                injected,
                                ..base.clone()
                            },
                        );
                    }
                }
                "thinking" => {
                    if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                        push(
                            result,
                            block_id,
                            CliSessionMessageRole::Assistant,
                            text.to_owned(),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Thinking,
                                ..base.clone()
                            },
                        );
                    }
                }
                "toolCall" => {
                    let call = block.get("toolCall");
                    let Some(call) = call else {
                        continue;
                    };
                    let name = call.get("name").and_then(Value::as_str).unwrap_or("工具");
                    let call_id = call
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| Some(format!("{id}:call:{index:04}")));
                    let arguments = call
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| {
                            call.get("input")
                                .and_then(Value::as_str)
                                .map(|value| Value::String(value.to_owned()))
                                .unwrap_or(Value::Null)
                        });
                    let arguments = match arguments {
                        Value::String(text) => serde_json::from_str(&text)
                            .unwrap_or(Value::String(text)),
                        value => value,
                    };
                    let mut metadata =
                        presentation::tool_call(name, &arguments, call_id.clone());
                    metadata.timestamp = base.timestamp.clone();
                    metadata.model = base.model.clone();
                    metadata.group_id = base.group_id.clone();
                    push(
                        result,
                        call_id
                            .map(|value| format!("mothx-call-{value}"))
                            .unwrap_or(block_id),
                        CliSessionMessageRole::Tool,
                        json_text(&arguments),
                        metadata,
                    );
                }
                "image" => {
                    if let Some(image) = block.get("image") {
                        let mime = image
                            .get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png");
                        push(
                            result,
                            block_id,
                            role_kind,
                            format!("![图片](data:{mime};base64,[inline image])"),
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Image,
                                ..base.clone()
                            },
                        );
                    }
                }
                "file" => {
                    if let Some(file) = block.get("file") {
                        let filename = file
                            .get("filename")
                            .or_else(|| file.get("title"))
                            .and_then(Value::as_str)
                            .unwrap_or("附件");
                        let text = file
                            .get("url")
                            .and_then(Value::as_str)
                            .filter(|url| !url.starts_with("data:"))
                            .map(|url| format!("[{filename}]({url})"))
                            .unwrap_or_else(|| format!("[附件] {filename}"));
                        push(
                            result,
                            block_id,
                            role_kind,
                            text,
                            SessionMessageMetadata {
                                kind: SessionMessageKind::Event,
                                ..base.clone()
                            },
                        );
                    }
                }
                "audio" | "video" => {
                    push(
                        result,
                        block_id,
                        role_kind,
                        format!("[{block_type}]"),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Event,
                            ..base.clone()
                        },
                    );
                }
                _ => {}
            }
        }
    }
    if let Some(attachments) = message.get("attachments").and_then(Value::as_array) {
        for (index, attachment) in attachments.iter().enumerate() {
            let name = attachment
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("附件");
            let url = attachment
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| !url.starts_with("data:"));
            let text = url
                .map(|url| format!("[{name}]({url})"))
                .unwrap_or_else(|| format!("[附件] {name}"));
            push(
                result,
                format!("{id}:attachment:{index:04}"),
                role_kind,
                text,
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    ..base.clone()
                },
            );
        }
    }
}

fn emit_tool_result(
    id: &str,
    message: &Value,
    base: &SessionMessageMetadata,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            message
                .get("contents")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| {
                            block
                                .get("text")
                                .and_then(Value::as_str)
                                .or_else(|| block.get("thinking").and_then(Value::as_str))
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|text| !text.trim().is_empty())
        });
    let Some(content) = content else {
        return;
    };
    let call_id = message
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let tool_name = message
        .get("toolName")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut metadata = presentation::tool_result(&Value::String(content.clone()), call_id);
    metadata.timestamp = base.timestamp.clone();
    metadata.model = base.model.clone();
    metadata.group_id = base.group_id.clone();
    metadata.tool_name = tool_name.filter(|name| !name.trim().is_empty());
    metadata.status = Some(
        if message.get("isError").and_then(Value::as_bool) == Some(true) {
            SessionToolStatus::Failed
        } else {
            SessionToolStatus::Succeeded
        },
    );
    push(
        result,
        format!("{id}:result"),
        CliSessionMessageRole::Tool,
        content,
        metadata,
    );
}

fn usage_from_message(
    id: &str,
    model: Option<&str>,
    message: &Value,
) -> Option<SessionTranscriptMutation> {
    let usage = message.get("usage")?;
    let counts = SessionTokenCounts {
        input_tokens: number(usage, "input"),
        output_tokens: number(usage, "output"),
        reasoning_tokens: number(usage, "reasoning"),
        cache_read_tokens: number(usage, "cacheRead"),
        cache_write_tokens: number(usage, "cacheWrite"),
    };
    if counts == SessionTokenCounts::default() {
        return None;
    }
    Some(usage::record(
        format!("mothx-usage-{id}"),
        model,
        counts,
        false,
        false,
        Some(id),
    ))
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
}

fn set_state(state: &mut Value, key: &str, value: &str) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    if let Some(object) = state.as_object_mut() {
        object.insert(key.into(), Value::String(value.to_owned()));
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
