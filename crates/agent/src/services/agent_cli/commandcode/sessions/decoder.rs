//! Command Code v3 transcript decoder.
//!
//! Each `message` entry wraps a Claude-compatible `{role, content}` body, so
//! the shared Claude record decoder handles text, thinking, tool calls and
//! tool results. The wrapper restores the top-level shape that decoder reads
//! and keeps title/model state from the surrounding entry types.

use crate::services::agent_cli::contracts::SessionTranscriptMutation;
use serde_json::{json, Value};
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
    match value.get("type").and_then(Value::as_str) {
        Some("message") => decode_message(&value, sequence, state),
        Some("session_info") => {
            if let Some(name) = value
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                set_state(state, "title", name);
            }
            Vec::new()
        }
        Some("model_change") => {
            if let Some(model) = value
                .get("model")
                .or_else(|| value.get("to"))
                .and_then(Value::as_str)
            {
                set_state(state, "model", model);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn decode_message(
    value: &Value,
    sequence: u64,
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    let Some(message) = value.get("message") else {
        return Vec::new();
    };
    let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
    if !matches!(role, "user" | "assistant") {
        return Vec::new();
    }
    if let Some(model) = value
        .get("model")
        .or_else(|| message.get("model"))
        .and_then(Value::as_str)
    {
        set_state(state, "model", model);
    }
    let injected = message
        .pointer("/meta/isMeta")
        .and_then(Value::as_bool)
        == Some(true);
    let transformed = json!({
        "type": role,
        "uuid": value.get("id"),
        "timestamp": value.get("timestamp"),
        "isMeta": injected,
        "message": {
            "role": role,
            "content": message.get("content"),
            "model": value
                .get("model")
                .or_else(|| message.get("model")),
            "usage": message.get("usage"),
        },
    });
    crate::services::agent_cli::claude::sessions::decoder::decode_record(
        "commandcode",
        &transformed,
        sequence,
        state,
    )
}

fn set_state(state: &mut Value, key: &str, value: &str) {
    if !state.is_object() {
        *state = json!({});
    }
    state[key] = Value::String(value.to_owned());
}
