use super::AgentCliKind;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslAgent {
    pub agent_kind: AgentCliKind,
    pub config_root: String,
    pub executable: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslDistribution {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub home: Option<String>,
    pub agents: Vec<WslAgent>,
    pub network_mode: Option<String>,
    pub host_gateway: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslInventory {
    pub supported: bool,
    pub distributions: Vec<WslDistribution>,
    pub error: Option<String>,
}
