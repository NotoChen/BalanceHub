use super::{
    AgentCliKind, AgentConfigurationDiagnosticSeverity, AgentConfigurationSnapshot,
    AgentHookHealthState, AgentSettingValueSummary, CliToolProbeResult,
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

/// 环境自检只回答“这台机器上现在能用什么”，不重复描述 Agent 的功能清单。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSelfCheckStatus {
    Ready,
    Degraded,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSelfCheckArea {
    Executable,
    Configuration,
    SessionHistory,
    Hook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSelfCheckIssue {
    pub area: AgentSelfCheckArea,
    pub severity: AgentConfigurationDiagnosticSeverity,
    pub message: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSelfCheckHook {
    pub state: AgentHookHealthState,
    pub installed: bool,
    pub enabled: bool,
    pub config_path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSelfCheckEntry {
    pub agent_kind: AgentCliKind,
    pub label: String,
    pub status: AgentSelfCheckStatus,
    pub executable: String,
    pub version: String,
    pub configuration_sources: usize,
    pub session_sources_total: usize,
    pub session_sources_available: usize,
    pub session_history: bool,
    pub hook_inventory: bool,
    pub hook: Option<AgentSelfCheckHook>,
    pub issues: Vec<AgentSelfCheckIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSelfCheckReport {
    pub checked_at: String,
    pub status: AgentSelfCheckStatus,
    pub ready: usize,
    pub degraded: usize,
    pub unavailable: usize,
    pub unknown: usize,
    pub entries: Vec<AgentSelfCheckEntry>,
}
