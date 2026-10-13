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
