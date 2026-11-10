//! GitHub Copilot CLI event-journal decoder.
//!
//! Each line is one persisted session event. Checkpoint copies and ephemeral
//! streaming deltas are skipped so the reader mirrors Copilot's own `history`
//! view. Reasoning is kept on its `assistant.reasoning` record; the copied
//! `reasoningText` on the assistant message is not repeated.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionTokenCounts,
        SessionToolStatus,
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
    _sequence: u64,
    line: &[u8],
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Vec::new();
    };
    // `ephemeral` records never reach the journal; `checkpoint` rows are replay
    // copies of original events and are excluded from the history view.
    if value.get("ephemeral").and_then(Value::as_bool) == Some(true)
        || value.get("checkpoint").and_then(Value::as_bool) == Some(true)
    {
        return Vec::new();
    }
    let event_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)));
    let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "session.start" | "session.resume" => {
            if let Some(model) = value
                .pointer("/data/selectedModel")
                .and_then(Value::as_str)
            {
                set_state(state, "model", model);
            }
            if kind == "session.start" {
                if let Some(version) = value
                    .pointer("/data/copilotVersion")
                    .and_then(Value::as_str)
                {
                    set_state(state, "version", version);
                }
            }
            Vec::new()
        }
        "session.model_change" => {
            if let Some(model) = value.pointer("/data/newModel").and_then(Value::as_str) {
                set_state(state, "model", model);
            }
            Vec::new()
        }
        "user.message" => decode_user_message(&value, &event_id, timestamp),
        "assistant.reasoning" => decode_reasoning(&value, &event_id, timestamp, state),
        "assistant.message" => decode_assistant_message(&value, timestamp, state),
        "tool.execution_start" => decode_tool_start(&value, timestamp, state),
        "tool.execution_complete" => decode_tool_complete(&value, timestamp),
        "session.compaction_complete" => decode_compaction(&value, &event_id, timestamp),
        "session.context_cleared" => {
            let cleared = value
                .pointer("/data/messagesCleared")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let text = if cleared > 0 {
                format!("上下文已清空（{cleared} 条消息）")
            } else {
                "上下文已清空".to_string()
            };
            vec![mutation(
                format!("copilot-context-cleared-{event_id}"),
                CliSessionMessageRole::Assistant,
                text,
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    timestamp,
                    ..Default::default()
                },
            )]
        }
        "session.truncation" => {
            let removed = value
                .pointer("/data/messagesRemovedDuringTruncation")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            vec![mutation(
                format!("copilot-truncation-{event_id}"),
                CliSessionMessageRole::Assistant,
                format!("已截断 {removed} 条消息以适配上下文"),
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    timestamp,
                    ..Default::default()
                },
            )]
        }
        "session.error" => {
            let message = value
                .pointer("/data/message")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("未知错误");
            vec![mutation(
                format!("copilot-error-{event_id}"),
                CliSessionMessageRole::Assistant,
                format!("[错误] {message}"),
                SessionMessageMetadata {
                    kind: SessionMessageKind::Event,
                    timestamp,
                    status: Some(SessionToolStatus::Failed),
                    ..Default::default()
                },
            )]
        }
        _ => Vec::new(),
    }
}

fn decode_user_message(
    value: &Value,
    event_id: &str,
    timestamp: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    let data = value.get("data").unwrap_or(&Value::Null);
    let message_id = data
        .get("messageId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(event_id);
    let mut content = data
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if content.trim().is_empty() {
        content = attachment_text(data.get("attachments"));
    }
    if content.trim().is_empty() {
        return Vec::new();
    }
    vec![mutation(
        format!("copilot-user-{message_id}"),
        CliSessionMessageRole::User,
        content,
        SessionMessageMetadata {
            kind: SessionMessageKind::Text,
            timestamp,
            group_id: Some(message_id.to_owned()),
            injected: data
                .get("isAutopilotContinuation")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..Default::default()
        },
    )]
}

fn decode_reasoning(
    value: &Value,
    event_id: &str,
    timestamp: Option<String>,
    state: &Value,
) -> Vec<SessionTranscriptMutation> {
    let data = value.get("data").unwrap_or(&Value::Null);
    let Some(content) = data
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Vec::new();
    };
    let reasoning_id = data
        .get("reasoningId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(event_id);
    let model = data
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| state_text(state, "model").map(str::to_owned));
    vec![mutation(
        format!("copilot-reasoning-{reasoning_id}"),
        CliSessionMessageRole::Assistant,
        content.to_owned(),
        SessionMessageMetadata {
            kind: SessionMessageKind::Thinking,
            timestamp,
            model,
            turn_id: data.get("turnId").and_then(Value::as_str).map(str::to_owned),
            group_id: data.get("turnId").and_then(Value::as_str).map(str::to_owned),
            ..Default::default()
        },
    )]
}

fn decode_assistant_message(
    value: &Value,
    timestamp: Option<String>,
    state: &Value,
) -> Vec<SessionTranscriptMutation> {
    let data = value.get("data").unwrap_or(&Value::Null);
    let Some(message_id) = data
        .get("messageId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Vec::new();
    };
    let model = data
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| state_text(state, "model").map(str::to_owned));
    let turn_id = data
        .get("turnId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut result = Vec::new();
    let mut text_id = None;
    if let Some(content) = data
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let id = format!("copilot-assistant-{message_id}");
        result.push(mutation(
            id.clone(),
            CliSessionMessageRole::Assistant,
            content.to_owned(),
            SessionMessageMetadata {
                kind: SessionMessageKind::Text,
                timestamp: timestamp.clone(),
                model: model.clone(),
                turn_id: turn_id.clone(),
                group_id: Some(message_id.to_owned()),
                ..Default::default()
            },
        ));
        text_id = Some(id);
    }
    if let Some(requests) = data.get("toolRequests").and_then(Value::as_array) {
        for request in requests {
            let Some(call_id) = request
                .get("toolCallId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            let name = request
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("工具");
            let arguments = request.get("arguments").cloned().unwrap_or(Value::Null);
            let mut metadata = presentation::tool_call(name, &arguments, Some(call_id.to_owned()));
            metadata.timestamp = timestamp.clone();
            metadata.model = model.clone();
            metadata.turn_id = turn_id.clone();
            metadata.group_id = turn_id.clone().or_else(|| Some(message_id.to_owned()));
            result.push(mutation(
                format!("copilot-call-{call_id}"),
                CliSessionMessageRole::Tool,
                json_text(&arguments),
                metadata,
            ));
        }
    }
    if let Some(output_tokens) = data.get("outputTokens").and_then(Value::as_u64) {
        result.push(usage::record(
            format!("copilot-usage-{message_id}"),
            model.as_deref(),
            SessionTokenCounts {
                output_tokens: Some(output_tokens),
                ..Default::default()
            },
            false,
            true,
            text_id.as_deref(),
        ));
    }
    result
}

fn decode_tool_start(
    value: &Value,
    timestamp: Option<String>,
    state: &Value,
) -> Vec<SessionTranscriptMutation> {
    let data = value.get("data").unwrap_or(&Value::Null);
    let Some(call_id) = data
        .get("toolCallId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Vec::new();
    };
    let name = data
        .get("toolName")
        .and_then(Value::as_str)
        .unwrap_or("工具");
    let arguments = data.get("arguments").cloned().unwrap_or(Value::Null);
    let mut metadata = presentation::tool_call(name, &arguments, Some(call_id.to_owned()));
    metadata.timestamp = timestamp;
    metadata.model = data
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| state_text(state, "model").map(str::to_owned));
    metadata.turn_id = data
        .get("turnId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    metadata.group_id = metadata.turn_id.clone();
    vec![mutation(
        format!("copilot-call-{call_id}"),
        CliSessionMessageRole::Tool,
        json_text(&arguments),
        metadata,
    )]
}

fn decode_tool_complete(value: &Value, timestamp: Option<String>) -> Vec<SessionTranscriptMutation> {
    let data = value.get("data").unwrap_or(&Value::Null);
    let Some(call_id) = data
        .get("toolCallId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Vec::new();
    };
    let success = data
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let error = data
        .pointer("/error/message")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let mut content = tool_result_text(data.get("result"));
    if content.trim().is_empty() {
        content = match error {
            Some(message) => format!("[失败] {message}"),
            None if success => "工具已执行完成".to_string(),
            None => "工具执行失败".to_string(),
        };
    }
    let exit_code = data
        .pointer("/shellExecution/exitCode")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| {
            data.pointer("/result/contents")
                .and_then(Value::as_array)
                .and_then(|contents| {
                    contents.iter().find_map(|content| {
                        (content.get("type").and_then(Value::as_str) == Some("shell_exit"))
                            .then(|| {
                                content
                                    .get("exitCode")
                                    .and_then(Value::as_i64)
                                    .and_then(|value| i32::try_from(value).ok())
                            })
                            .flatten()
                    })
                })
        });
    let mut metadata = SessionMessageMetadata {
        kind: SessionMessageKind::ToolResult,
        call_id: Some(call_id.to_owned()),
        status: Some(if success {
            SessionToolStatus::Succeeded
        } else {
            SessionToolStatus::Failed
        }),
        exit_code,
        timestamp,
        ..Default::default()
    };
    metadata.group_id = data
        .get("turnId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    vec![mutation(
        format!("copilot-result-{call_id}"),
        CliSessionMessageRole::Tool,
        content,
        metadata,
    )]
}

fn decode_compaction(
    value: &Value,
    event_id: &str,
    timestamp: Option<String>,
) -> Vec<SessionTranscriptMutation> {
    let data = value.get("data").unwrap_or(&Value::Null);
    let success = data
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let content = if success {
        data.get("summaryContent")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| Some("上下文已压缩".to_string()))
    } else {
        let reason = data
            .get("failureReason")
            .or_else(|| data.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("未知原因");
        Some(format!("[压缩失败] {reason}"))
    };
    let Some(content) = content else {
        return Vec::new();
    };
    vec![mutation(
        format!("copilot-compaction-{event_id}"),
        CliSessionMessageRole::Assistant,
        content,
        SessionMessageMetadata {
            kind: SessionMessageKind::Event,
            timestamp,
            ..Default::default()
        },
    )]
}

fn tool_result_text(result: Option<&Value>) -> String {
    let Some(result) = result else {
        return String::new();
    };
    if let Some(content) = result
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return content.to_owned();
    }
    let mut parts = Vec::new();
    if let Some(contents) = result.get("contents").and_then(Value::as_array) {
        for content in contents {
            match content.get("type").and_then(Value::as_str) {
                Some("text") | Some("terminal") => {
                    if let Some(text) = content.get("text").and_then(Value::as_str) {
                        if !text.trim().is_empty() {
                            parts.push(text.to_owned());
                        }
                    }
                }
                Some("image") => {
                    let mime = content
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png");
                    parts.push(format!("![图片](data:{mime};base64,[inline image])"));
                }
                Some("shell_exit") => {
                    if let Some(code) = content.get("exitCode").and_then(Value::as_i64) {
                        parts.push(format!("[退出码 {code}]"));
                    }
                }
                _ => {}
            }
        }
    }
    parts.join("\n")
}

fn attachment_text(attachments: Option<&Value>) -> String {
    let Some(attachments) = attachments.and_then(Value::as_array) else {
        return String::new();
    };
    let names = attachments
        .iter()
        .filter_map(|attachment| {
            ["displayName", "path", "filePath", "text"]
                .iter()
                .find_map(|key| attachment.get(*key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        String::new()
    } else {
        names.join("\n")
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

fn set_state(state: &mut Value, key: &str, value: &str) {
    if !state.is_object() {
        *state = Value::Object(serde_json::Map::new());
    }
    state[key] = Value::String(value.to_owned());
}

fn state_text<'a>(state: &'a Value, key: &str) -> Option<&'a str> {
    state.get(key).and_then(Value::as_str)
}
