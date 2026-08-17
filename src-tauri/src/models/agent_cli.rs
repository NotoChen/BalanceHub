use serde::Serialize;

use super::AgentCliKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCliCapabilities {
    pub temporary_launch: bool,
    pub model_selection: bool,
    pub session_history: bool,
    pub session_resume: bool,
    pub session_name: bool,
    pub liveness: bool,
    pub default_config: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCliDescriptor {
    pub kind: AgentCliKind,
    pub label: String,
    pub executable: String,
    pub session_name_hint: String,
    pub capabilities: AgentCliCapabilities,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliToolProbeResult {
    #[serde(flatten)]
    pub descriptor: AgentCliDescriptor,
    pub available: bool,
    pub path: String,
    pub version: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEnvironmentProbeResult {
    pub tools: Vec<CliToolProbeResult>,
}
