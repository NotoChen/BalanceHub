//! Same normalized TOML identity used by the pinned Codex native trust hash.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(in crate::services::agent_cli::codex::environment) fn normalized_hash(
    event: &str,
    group: &Value,
    handler: &Value,
) -> Option<String> {
    let mut normalized = Map::new();
    let kind = handler.get("type")?.as_str()?;
    normalized.insert("type".to_owned(), Value::String(kind.to_owned()));
    if kind == "command" {
        let command = if cfg!(windows) {
            handler
                .get("commandWindows")
                .or_else(|| handler.get("command_windows"))
                .and_then(Value::as_str)
                .or_else(|| handler.get("command").and_then(Value::as_str))
        } else {
            handler.get("command").and_then(Value::as_str)
        }?;
        normalized.insert("command".to_owned(), Value::String(command.to_owned()));
        normalized.insert(
            "async".to_owned(),
            Value::Bool(
                handler
                    .get("async")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        );
        if matches!(
            event,
            "PreToolUse" | "PostToolUse" | "SessionStart" | "UserPromptSubmit" | "SubagentStart"
        ) {
            if let Some(limit) = handler
                .get("additionalContextLimit")
                .and_then(Value::as_u64)
                .filter(|limit| *limit != 2500)
            {
                normalized.insert("additionalContextLimit".to_owned(), Value::from(limit));
            }
        }
    } else if kind == "mcp_tool" {
        normalized.insert("server".to_owned(), handler.get("server")?.clone());
        normalized.insert("tool".to_owned(), handler.get("tool")?.clone());
        normalized.insert(
            "input".to_owned(),
            handler
                .get("input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new())),
        );
    } else {
        return None;
    }
    let timeout = handler.get("timeout").and_then(Value::as_u64);
    let timeout = if matches!(event, "SessionEnd" | "Interrupt") {
        timeout.unwrap_or(1).clamp(1, 3)
    } else {
        timeout.unwrap_or(600).max(1)
    };
    normalized.insert("timeout".to_owned(), Value::from(timeout));
    if let Some(message) = handler.get("statusMessage").and_then(Value::as_str) {
        normalized.insert(
            "statusMessage".to_owned(),
            Value::String(message.to_owned()),
        );
    }
    let mut identity = Map::new();
    identity.insert(
        "event_name".to_owned(),
        Value::String(super::event_key(event)?.to_owned()),
    );
    if let Some(matcher) = super::effective_matcher(event, group) {
        identity.insert("matcher".to_owned(), Value::String(matcher.to_owned()));
    }
    identity.insert(
        "hooks".to_owned(),
        Value::Array(vec![Value::Object(normalized)]),
    );
    // Sort explicitly: Cargo feature unification may enable preserve_order in
    // serde_json. Native fingerprints sort every object, including MCP input.
    let bytes = serde_json::to_vec(&canonical_json(&Value::Object(identity))).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical_json(&object[key])))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        value => value.clone(),
    }
}
