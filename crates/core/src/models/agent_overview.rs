use super::{
    AgentCliKind, AgentConfigurationSnapshot, AgentSettingValueSummary, CliToolProbeResult,
};
use serde::{Deserialize, Serialize};

/// Display only. Resource counts/installations come from the shared catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOverviewSnapshot {
    pub agent_kind: AgentCliKind,
    pub probe: CliToolProbeResult,
    pub configuration: Option<AgentConfigurationSnapshot>,
    pub configuration_error: String,
    /// Native default model, resolved the same way as the visual settings form.
    pub model: AgentSettingValueSummary,
    pub updated_at: String,
}
