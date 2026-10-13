//! Small numeric facts emitted by native decoders in the existing indexing pass.
use crate::{models::*, services::agent_cli::contracts::SessionIndexMutation};
use serde_json::Value;

pub(crate) fn number(value: &Value, name: &str) -> Option<u64> {
    value.get(name).and_then(Value::as_u64)
}

pub(crate) fn record(
    id: String,
    model: Option<&str>,
    tokens: SessionTokenCounts,
    cumulative: bool,
    uncertain: bool,
    message_id: Option<&str>,
) -> SessionIndexMutation {
    SessionIndexMutation::Usage(SessionUsageRecord {
        id,
        message_id: message_id.map(str::to_owned),
        model: model.map(|model| model.chars().take(256).collect()),
        tokens,
        cumulative,
        uncertain,
    })
}

impl SessionTokenCounts {
    pub(crate) fn add(&mut self, other: &Self, first: bool) {
        for (target, value) in [
            (&mut self.input_tokens, other.input_tokens),
            (&mut self.output_tokens, other.output_tokens),
            (&mut self.cache_read_tokens, other.cache_read_tokens),
            (&mut self.cache_write_tokens, other.cache_write_tokens),
            (&mut self.reasoning_tokens, other.reasoning_tokens),
        ] {
            *target = if first {
                value
            } else {
                target
                    .zip(value)
                    .map(|(total, value)| total.saturating_add(value))
            };
        }
    }
    pub(crate) fn retain_maximum(&mut self, previous: &Self) -> bool {
        let mut decreased = false;
        for (target, old) in [
            (&mut self.input_tokens, previous.input_tokens),
            (&mut self.output_tokens, previous.output_tokens),
            (&mut self.cache_read_tokens, previous.cache_read_tokens),
            (&mut self.cache_write_tokens, previous.cache_write_tokens),
            (&mut self.reasoning_tokens, previous.reasoning_tokens),
        ] {
            if let Some(old) = old {
                decreased |= target.is_some_and(|current| current < old);
                *target = Some(target.unwrap_or_default().max(old));
            }
        }
        decreased
    }
}
