use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpTool {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpCheck {
    pub id: String,
    pub phase: String,
    pub outcome: Option<String>,
    pub message: String,
    pub server_name: Option<String>,
    pub protocol_version: Option<String>,
    pub tools: Vec<AgentMcpTool>,
    pub elapsed_ms: u64,
}
