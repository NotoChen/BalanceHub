//! Mister Morph task-journal decoder.
//!
//! `journal_events.payload` is raw-deflate compressed JSON (`task.JournalPayload`),
//! stored as a BLOB; the transcript query returns it hex-encoded so the shared
//! SQLite reader can pass it through as text. Each task snapshot carries one
//! user turn, its retained trace and, once finished, the final answer.

use crate::{
    models::{
        CliSessionMessageRole, SessionMessageKind, SessionMessageMetadata, SessionToolStatus,
    },
    services::{
        agent_cli::contracts::{SessionTranscriptMessage, SessionTranscriptMutation},
        cli_sessions::{json_text, normalize_timestamp, presentation},
    },
};
use flate2::read::DeflateDecoder;
use serde_json::Value;
use std::{
    io::Read,
    path::Path,
};

const MAX_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn decode(
    _path: &Path,
    _sequence: u64,
    line: &[u8],
    _state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Ok(text) = std::str::from_utf8(line) else {
        return Vec::new();
    };
    let Some(payload) = payload_hex(text) else {
        return Vec::new();
    };
    let Some(task) = payload.get("task") else {
        return Vec::new();
    };
    let Some(task_id) = task
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let model = task
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let created_at = timestamp(task.get("created_at"));
    let finished_at = timestamp(task.get("finished_at")).or_else(|| timestamp(payload.get("at")));
    let mut result = Vec::new();
    if let Some(prompt) = task
        .get("task")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        result.push(mutation(
            format!("morph-{task_id}:user"),
            CliSessionMessageRole::User,
            prompt.to_owned(),
            SessionMessageMetadata {
                kind: SessionMessageKind::Text,
                timestamp: created_at,
                turn_id: Some(task_id.to_owned()),
                group_id: Some(task_id.to_owned()),
                ..Default::default()
            },
        ));
    }
    let task_result = task.get("result");
    let final_answer = task_result.and_then(|value| value.get("final"));
    decode_trace(task_result, task_id, model.as_deref(), &mut result);
    if let Some(thinking) = final_answer
        .and_then(|value| value.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        result.push(mutation(
            format!("morph-{task_id}:answer-thinking"),
            CliSessionMessageRole::Assistant,
            thinking.to_owned(),
            SessionMessageMetadata {
                kind: SessionMessageKind::Thinking,
                timestamp: finished_at.clone(),
                model: model.clone(),
                turn_id: Some(task_id.to_owned()),
                group_id: Some(task_id.to_owned()),
                ..Default::default()
            },
        ));
    }
    let answer = final_answer
        .and_then(|value| output_text(value.get("output")))
        .or_else(|| {
            task_result
                .filter(|value| !value.is_object())
                .and_then(|value| output_text(Some(value)))
        });
    if let Some(answer) = answer {
        result.push(mutation(
            format!("morph-{task_id}:answer"),
            CliSessionMessageRole::Assistant,
            answer,
            SessionMessageMetadata {
                kind: SessionMessageKind::Text,
                timestamp: finished_at.clone(),
                model: model.clone(),
                turn_id: Some(task_id.to_owned()),
                group_id: Some(task_id.to_owned()),
                ..Default::default()
            },
        ));
    }
    let status = task.get("status").and_then(Value::as_str).unwrap_or_default();
    let status_text = match status {
        "failed" => Some((
            SessionToolStatus::Failed,
            task.get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("[失败] {value}"))
                .unwrap_or_else(|| "任务执行失败".to_string()),
        )),
        "canceled" => Some((SessionToolStatus::Cancelled, "任务已取消".to_string())),
        _ => None,
    };
    if let Some((status, text)) = status_text {
        result.push(mutation(
            format!("morph-{task_id}:status"),
            CliSessionMessageRole::Assistant,
            text,
            SessionMessageMetadata {
                kind: SessionMessageKind::Event,
                timestamp: finished_at,
                status: Some(status),
                turn_id: Some(task_id.to_owned()),
                group_id: Some(task_id.to_owned()),
                ..Default::default()
            },
        ));
    }
    result
}

fn decode_trace(
    task_result: Option<&Value>,
    task_id: &str,
    model: Option<&str>,
    result: &mut Vec<SessionTranscriptMutation>,
) {
    let Some(entries) = task_result
        .and_then(|value| value.pointer("/trace/entries"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for entry in entries {
        let Some(event) = entry.get("event") else {
            continue;
        };
        let sequence = entry.get("seq").and_then(Value::as_u64).unwrap_or(0);
        let timestamp = timestamp(entry.get("at"));
        let run_id = event
            .get("run_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| task_id.to_owned());
        let event_model = event
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or(model);
        if let Some(plan) = entry.get("plan") {
            if let Some(reasoning) = plan
                .get("reasoning")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                result.push(mutation(
                    format!("morph-{task_id}:plan-thinking"),
                    CliSessionMessageRole::Assistant,
                    reasoning.to_owned(),
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Thinking,
                        timestamp: timestamp.clone(),
                        model: event_model.map(str::to_owned),
                        turn_id: Some(run_id.clone()),
                        group_id: Some(task_id.to_owned()),
                        ..Default::default()
                    },
                ));
            }
            if let Some(steps) = plan.get("steps").and_then(Value::as_array) {
                for (index, step) in steps.iter().enumerate() {
                    let Some(note) = step
                        .get("note")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                    else {
                        continue;
                    };
                    result.push(mutation(
                        format!("morph-{task_id}:step-{index}"),
                        CliSessionMessageRole::Assistant,
                        note.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Text,
                            timestamp: timestamp.clone(),
                            model: event_model.map(str::to_owned),
                            turn_id: Some(run_id.clone()),
                            group_id: Some(task_id.to_owned()),
                            ..Default::default()
                        },
                    ));
                }
            }
        }
        match event.get("kind").and_then(Value::as_str) {
            Some("insight") => {
                if let Some(text) = event
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    result.push(mutation(
                        format!("morph-{task_id}:insight-{sequence}"),
                        CliSessionMessageRole::Assistant,
                        text.to_owned(),
                        SessionMessageMetadata {
                            kind: SessionMessageKind::Text,
                            timestamp: timestamp.clone(),
                            model: event_model.map(str::to_owned),
                            turn_id: Some(run_id.clone()),
                            group_id: Some(task_id.to_owned()),
                            ..Default::default()
                        },
                    ));
                }
            }
            Some("tool_start") => {
                let call_id = event
                    .get("activity_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{task_id}-{sequence}"));
                let name = event
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("工具");
                let arguments = Value::Object(
                    event
                        .get("args")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default(),
                );
                let mut metadata =
                    presentation::tool_call(name, &arguments, Some(call_id.clone()));
                metadata.timestamp = timestamp;
                metadata.model = event_model.map(str::to_owned);
                metadata.turn_id = Some(run_id);
                metadata.group_id = Some(task_id.to_owned());
                metadata.status = Some(SessionToolStatus::Running);
                result.push(mutation(
                    format!("morph-call-{call_id}"),
                    CliSessionMessageRole::Tool,
                    json_text(&arguments),
                    metadata,
                ));
            }
            Some("tool_done") => {
                let call_id = event
                    .get("activity_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{task_id}-{sequence}"));
                let status = tool_status(event.get("status").and_then(Value::as_str));
                let error = event
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let content = event
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        event
                            .get("summary")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                    })
                    .map(str::to_owned)
                    .or_else(|| error.map(|value| format!("[失败] {value}")))
                    .unwrap_or_else(|| match status {
                        Some(SessionToolStatus::Failed) => "工具执行失败".to_string(),
                        Some(SessionToolStatus::Cancelled) => "工具调用已取消".to_string(),
                        _ => "工具已执行完成".to_string(),
                    });
                result.push(mutation(
                    format!("morph-result-{call_id}"),
                    CliSessionMessageRole::Tool,
                    content,
                    SessionMessageMetadata {
                        kind: SessionMessageKind::ToolResult,
                        timestamp,
                        model: event_model.map(str::to_owned),
                        turn_id: Some(run_id),
                        group_id: Some(task_id.to_owned()),
                        call_id: Some(call_id),
                        status,
                        ..Default::default()
                    },
                ));
            }
            Some("context_compaction_done" | "context_compaction_failed") => {
                let failed = event.get("kind").and_then(Value::as_str)
                    == Some("context_compaction_failed");
                let content = event
                    .get("summary")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .or_else(|| {
                        event
                            .get("error")
                            .and_then(Value::as_str)
                            .map(|value| format!("[压缩失败] {value}"))
                    })
                    .unwrap_or_else(|| {
                        if failed {
                            "上下文压缩失败".to_string()
                        } else {
                            "上下文已压缩".to_string()
                        }
                    });
                result.push(mutation(
                    format!("morph-{task_id}:compaction-{sequence}"),
                    CliSessionMessageRole::Assistant,
                    content,
                    SessionMessageMetadata {
                        kind: SessionMessageKind::Event,
                        timestamp,
                        status: failed.then_some(SessionToolStatus::Failed),
                        turn_id: Some(run_id),
                        group_id: Some(task_id.to_owned()),
                        ..Default::default()
                    },
                ));
            }
            _ => {}
        }
    }
}

pub(super) fn payload_hex(text: &str) -> Option<Value> {
    let bytes = hex_decode(text.trim())?;
    let mut reader = DeflateDecoder::new(bytes.as_slice()).take(MAX_DECOMPRESSED_BYTES as u64 + 1);
    let mut raw = Vec::new();
    reader.read_to_end(&mut raw).ok()?;
    if raw.len() > MAX_DECOMPRESSED_BYTES {
        return None;
    }
    serde_json::from_slice(&raw).ok()
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let (pairs, _) = bytes.as_chunks::<2>();
    for pair in pairs {
        out.push(nibble(pair[0])? << 4 | nibble(pair[1])?);
    }
    Some(out)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn tool_status(raw: Option<&str>) -> Option<SessionToolStatus> {
    match raw? {
        "completed" | "succeeded" | "success" | "done" => Some(SessionToolStatus::Succeeded),
        "failed" | "error" => Some(SessionToolStatus::Failed),
        "canceled" | "cancelled" => Some(SessionToolStatus::Cancelled),
        "running" | "started" | "in_progress" | "pending" => Some(SessionToolStatus::Running),
        _ => None,
    }
}

fn output_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) if text.trim().is_empty() => None,
        Value::String(text) => Some(text.clone()),
        other => Some(json_text(other)),
    }
}

fn timestamp(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .and_then(|value| normalize_timestamp(Some(value)))
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
