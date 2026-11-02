use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionTokenCounts {
    /// Input includes cache reads/writes; reasoning is part of output.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageRecord {
    pub id: String,
    /// Snapshot-based logs associate usage with a replaceable/rewindable message.
    pub message_id: Option<String>,
    pub model: Option<String>,
    pub tokens: SessionTokenCounts,
    pub cumulative: bool,
    pub uncertain: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelUsage {
    pub model: Option<String>,
    pub tokens: SessionTokenCounts,
    pub records: u64,
    pub estimated_cost_microusd: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionUsageSummary {
    pub tokens: SessionTokenCounts,
    pub models: Vec<SessionModelUsage>,
    pub partial: bool,
    pub native_cumulative: bool,
    pub records: u64,
    pub estimated_cost_microusd: Option<u64>,
    pub unpriced_records: u64,
}

impl SessionTokenCounts {
    pub fn add(&mut self, other: &Self, first: bool) {
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
    pub fn retain_maximum(&mut self, previous: &Self) -> bool {
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
