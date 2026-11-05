//! Small numeric facts emitted by native decoders in the existing indexing pass.
use crate::{models::*, services::agent_cli::contracts::SessionTranscriptMutation};
use serde_json::Value;

pub fn number(value: &Value, name: &str) -> Option<u64> {
    value.get(name).and_then(Value::as_u64)
}

pub fn record(
    id: String,
    model: Option<&str>,
    tokens: SessionTokenCounts,
    cumulative: bool,
    uncertain: bool,
    message_id: Option<&str>,
) -> SessionTranscriptMutation {
    SessionTranscriptMutation::Usage(SessionUsageRecord {
        id,
        message_id: message_id.map(str::to_owned),
        model: model.map(|model| model.chars().take(256).collect()),
        tokens,
        cumulative,
        uncertain,
    })
}
