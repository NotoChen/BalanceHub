//! Parse dotenv assignments as data without executing or rewriting the file.
use crate::{
    models::{AgentConfigurationError, AgentConfigurationErrorKind},
    services::agent_cli::config_support::env_source,
};
use serde_json::Value;

pub(super) fn parse_env(text: &str) -> Result<Value, AgentConfigurationError> {
    if text.contains('\0') {
        return Err(invalid());
    }
    let assignments = env_source::assignments(text);
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let clean = line.trim();
        if !clean.is_empty()
            && !clean.starts_with('#')
            && !assignments.iter().any(|assignment| {
                assignment.value_start >= offset && assignment.value_start <= offset + line.len()
            })
        {
            return Err(invalid());
        }
        offset += line.len();
    }
    let mut value = serde_json::Map::new();
    for assignment in assignments {
        if value.contains_key(&assignment.key) {
            return Err(invalid());
        }
        let raw = &text[assignment.value_start..assignment.value_end];
        if raw.starts_with(['\'', '"', '`'])
            && (raw.len() < 2 || raw.as_bytes().first() != raw.as_bytes().last())
        {
            return Err(invalid());
        }
        let tail_end = text[assignment.value_end..]
            .find('\n')
            .map_or(text.len(), |index| assignment.value_end + index);
        let tail = text[assignment.value_end..tail_end].trim();
        if !tail.is_empty() && !tail.starts_with('#') {
            return Err(invalid());
        }
        value.insert(assignment.key, Value::String(assignment.value));
    }
    Ok(Value::Object(value))
}

fn invalid() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidSyntax)
}
