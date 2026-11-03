//! Gemini 0.59 Hook schema shared by passive observation and catalog edits.
//! Matcher identities stay private; neither diagnostics nor labels contain them.

use serde_json::{Map, Value};

pub(in super::super) fn valid_event(event: &str) -> bool {
    matches!(
        event,
        "BeforeTool"
            | "AfterTool"
            | "BeforeAgent"
            | "Notification"
            | "AfterAgent"
            | "SessionStart"
            | "SessionEnd"
            | "PreCompress"
            | "BeforeModel"
            | "AfterModel"
            | "BeforeToolSelection"
    )
}

pub(in super::super) fn disable_identity(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    match object.get("type").and_then(Value::as_str)? {
        "command" => {
            let command = object
                .get("command")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())?;
            let name = match object.get("name") {
                None => None,
                Some(Value::String(name)) => Some(name.as_str()),
                Some(_) => return None,
            };
            Some(name.filter(|value| !value.is_empty()).unwrap_or(command))
        }
        "runtime" => object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty()),
        "plugin" => match object.get("name") {
            None => Some("unknown-hook"),
            Some(Value::String(name)) if !name.is_empty() => Some(name),
            Some(Value::String(_)) => Some("unknown-hook"),
            Some(_) => None,
        },
        _ => None,
    }
}

pub(in super::super) fn disabled_names(
    root: &Map<String, Value>,
) -> Result<Option<Vec<&str>>, &'static str> {
    let Some(config) = root.get("hooksConfig") else {
        return Ok(None);
    };
    let config = config.as_object().ok_or("hooksConfig")?;
    let Some(disabled) = config.get("disabled") else {
        return Ok(None);
    };
    let values = disabled.as_array().ok_or("hooksConfig.disabled")?;
    values
        .iter()
        .map(|value| value.as_str().ok_or("hooksConfig.disabled"))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(in super::super) fn valid_group(group: &Value) -> bool {
    group.as_object().is_some_and(|object| {
        object.get("matcher").is_none_or(Value::is_string)
            && object.get("sequential").is_none_or(Value::is_boolean)
            && object.get("hooks").is_some_and(Value::is_array)
    })
}

pub(in super::super) fn sequential_siblings(group: &Value) -> bool {
    group.get("sequential").and_then(Value::as_bool) == Some(true)
        && group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|handlers| handlers.len() > 1)
}

pub(in super::super) fn same_group_metadata(left: &Value, right: &Value) -> bool {
    let metadata = |value: &Value| {
        value.as_object().map(|object| {
            object
                .iter()
                .filter(|(key, _)| key.as_str() != "hooks")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>()
        })
    };
    metadata(left)
        .zip(metadata(right))
        .is_some_and(|(left, right)| left == right)
}
