use serde::{Deserialize, Serialize};

use super::AgentCliKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCliCapabilities {
    pub temporary_launch: bool,
    pub model_selection: bool,
    pub session_history: bool,
    pub session_search: bool,
    pub session_detail: bool,
    pub session_resume: bool,
    pub session_name: bool,
    pub liveness: bool,
    pub default_config: bool,
    /// Native Skill / MCP / plugin inventory. Integration-tier Agents can
    /// still edit native configuration without publishing an asset catalog.
    pub asset_inventory: bool,
    /// Native Hook definitions exposed by the Agent's catalog adapter.
    pub hook_inventory: bool,
    /// BalanceHub can observe live sessions through managed hooks or launches
    /// it started itself. CLI liveness probing is not session observation.
    pub runtime_observation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCliDescriptor {
    pub kind: AgentCliKind,
    pub label: String,
    pub executable: String,
    pub session_name_hint: String,
    pub capabilities: AgentCliCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolProbeResult {
    #[serde(flatten)]
    pub descriptor: AgentCliDescriptor,
    pub available: bool,
    pub path: String,
    pub version: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEnvironmentProbeResult {
    pub tools: Vec<CliToolProbeResult>,
}
