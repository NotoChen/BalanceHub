//! Goose message-row decoder.
//!
//! One row is one stored message whose `content` is a block array in the MCP
//! content shape Goose uses for both model and tool traffic. Tool requests and
//! responses are split into separate rows so the reader can pair them by call
//! id while text and thinking keep their native order.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{clean_text, json_text, presentation, timestamp_from_value},
    },
};
use serde_json::Value;
use std::path::Path;

const MILLISECOND_THRESHOLD: i64 = 10_000_000_000;

pub(super) fn decode(
    _path: &Path,
    sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if role == "system" {
        return Vec::new();
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("row_id")
                .and_then(Value::as_i64)
                .map(|id| format!("row-{id}"))
        })
        .unwrap_or_else(|| format!("goose-{sequence}"));
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_i64)
        .and_then(|value| timestamp_from_value(Some(value), value.abs() > MILLISECOND_THRESHOLD));
    let base = SessionMessageMetadata {
        timestamp,
        group_id: Some(id.clone()),
        ..Default::default()
    };
    let role_kind = if role == "user" {
        CliSessionMessageRole::User
    } else {
        CliSessionMessageRole::Assistant
    };
    let mut result = Vec::new();
    for (index, block) in blocks(value.get("content")).iter().enumerate() {
        let index = index as u32;
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    push(
                        &mut result,
                        format!("goose-{id}:{index:04}"),
                        role_kind,
                        text.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Text,
                            ..base.clone()
                        },
                    );
                }
            }
            "thinking" => {
                if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                    push(
                        &mut result,
                        format!("goose-{id}:{index:04}"),
                        CliSessionMessageRole::Assistant,
                        text.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Thinking,
                            ..base.clone()
                        },
                    );
                }
            }
            "image" => {
                let mime = block
                    .get("mimeType")
                    .or_else(|| block.get("mime_type"))
                    .and_then(Value::as_str)
                    .unwrap_or("image/png");
                let label = clean_text(
                    block
                        .get("_meta")
                        .and_then(|meta| meta.get("title"))
                        .and_then(Value::as_str)
                        .unwrap_or("图片"),
                    100,
                )
                .unwrap_or_else(|| "图片".into());
                push(
                    &mut result,
                    format!("goose-{id}:{index:04}"),
                    role_kind,
                    [format!("![{label}](data:{mime};base64,[inline image])")].join("\n"),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Image,
                        ..base.clone()
                    },
                );
            }
            "toolRequest" => {
                let call = block.get("toolCall");
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let failed = call
                    .and_then(|call| call.get("status"))
                    .and_then(Value::as_str)
                    .is_some_and(|status| status == "error");
                let Some(call_value) = call.and_then(|call| call.get("value")) else {
                    continue;
                };
                let name = call_value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("工具");
                let arguments = call_value.get("arguments").cloned().unwrap_or(Value::Null);
                let mut metadata = presentation::tool_call(name, &arguments, call_id.clone());
                metadata.timestamp = base.timestamp.clone();
                metadata.group_id = base.group_id.clone();
                if failed {
                    metadata.status = Some(SessionToolStatus::Failed);
                }
                push(
                    &mut result,
                    call_id
                        .map(|id| format!("goose-call-{id}"))
                        .unwrap_or_else(|| format!("goose-{id}:call:{index:04}")),
                    CliSessionMessageRole::Tool,
                    json_text(&arguments),
                    metadata,
                );
            }
            "toolResponse" => {
                let result_value = block.get("toolResult");
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let failed = result_value
                    .and_then(|result| result.get("status"))
                    .and_then(Value::as_str)
                    .is_some_and(|status| status == "error");
                let content = result_value
                    .and_then(|result| result.get("value"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let text = render_content(&content);
                let mut metadata = presentation::tool_result(&content, call_id.clone());
                metadata.timestamp = base.timestamp.clone();
                metadata.group_id = base.group_id.clone();
                if failed {
                    metadata.status = Some(SessionToolStatus::Failed);
                }
                push(
                    &mut result,
                    call_id
                        .map(|id| format!("goose-result-{id}"))
                        .unwrap_or_else(|| format!("goose-{id}:result:{index:04}")),
                    CliSessionMessageRole::Tool,
                    text,
                    metadata,
                );
            }
            "systemNotification" => {
                let text = block
                    .get("notificationType")
                    .or_else(|| block.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("系统通知");
                push(
                    &mut result,
                    format!("goose-{id}:{index:04}"),
                    CliSessionMessageRole::Assistant,
                    format!("[系统] {text}"),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Event,
                        injected: true,
                        ..base.clone()
                    },
                );
            }
            "error" => {
                let text = block
                    .get("error")
                    .or_else(|| block.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                push(
                    &mut result,
                    format!("goose-{id}:{index:04}"),
                    CliSessionMessageRole::Assistant,
                    format!("[错误] {text}"),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Event,
                        status: Some(SessionToolStatus::Failed),
                        ..base.clone()
                    },
                );
            }
            "toolConfirmationRequest" | "actionRequired" => {
                let text = block
                    .get("text")
                    .or_else(|| block.get("prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or("等待工具确认");
                push(
                    &mut result,
                    format!("goose-{id}:{index:04}"),
                    CliSessionMessageRole::Assistant,
                    format!("[确认] {text}"),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Event,
                        ..base.clone()
                    },
                );
            }
            _ => {}
        }
    }
    result
}

fn blocks(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Object(_)) => vec![value.cloned().unwrap_or_default()],
        Some(Value::String(text)) => {
            match serde_json::from_str::<Value>(text) {
                Ok(Value::Array(items)) => items,
                _ => vec![serde_json::json!({ "type": "text", "text": text })],
            }
        }
        _ => Vec::new(),
    }
}

fn render_content(content: &Value) -> String {
    let items = match content {
        Value::Array(items) => items.clone(),
        Value::String(text) => return text.clone(),
        _ => Vec::new(),
    };
    items
        .iter()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("text") => item.get("text").and_then(Value::as_str).map(str::to_owned),
            Some("image") => {
                let mime = item
                    .get("mimeType")
                    .or_else(|| item.get("mime_type"))
                    .and_then(Value::as_str)
                    .unwrap_or("image/png");
                Some(format!("![图片](data:{mime};base64,[inline image])"))
            }
            Some("resource") => item.get("resource").map(json_text),
            Some(_) => Some(json_text(item)),
            None => item.as_str().map(str::to_owned),
        })
        .collect::<Vec<_>>()
        .join("\n")
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
