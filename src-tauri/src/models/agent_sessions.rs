use serde::{Deserialize, Serialize};

use super::{AgentCliKind, CliSessionDetail, CliSessionIndexState, CliSessionSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionWorkspace {
    pub id: String,
    pub path: String,
    pub exists: bool,
    pub is_home: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSource {
    pub id: String,
    pub agent_kind: AgentCliKind,
    pub config_root: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionScope {
    pub revision: String,
    pub workspaces: Vec<AgentSessionWorkspace>,
    pub sources: Vec<AgentSessionSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionRole {
    Main,
    Subagent,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionRoleFilter {
    #[default]
    All,
    Main,
    Subagent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentSessionParent {
    None,
    Known {
        native_id: String,
        parent_ref: Option<String>,
    },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionActivityState {
    Unknown,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRow {
    pub session_ref: String,
    pub source_id: String,
    pub workspace_id: String,
    pub session: CliSessionSummary,
    pub role: AgentSessionRole,
    pub parent: AgentSessionParent,
    pub resume_reason: Option<String>,
    pub activity_state: AgentSessionActivityState,
    pub runtime_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionQuery {
    pub consumer_id: String,
    pub request_id: u64,
    pub scope_revision: String,
    pub agent_kinds: Vec<AgentCliKind>,
    pub workspace_ids: Vec<String>,
    #[serde(default)]
    pub role_filter: AgentSessionRoleFilter,
    #[serde(default)]
    pub query: String,
    pub page_size: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionSourceStatus {
    Complete,
    Partial,
    Unavailable,
    Unsupported,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSourceState {
    pub source_id: String,
    pub workspace_id: String,
    pub state: AgentSessionSourceStatus,
    pub loaded_count: usize,
    pub message: Option<String>,
    pub index_state: CliSessionIndexState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionParentUpdate {
    pub session_ref: String,
    pub parent: AgentSessionParent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCount {
    pub agent_kind: AgentCliKind,
    pub loaded_count: usize,
    pub total: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCountRequest {
    pub consumer_id: String,
    pub request_id: u64,
    pub agent_kinds: Vec<AgentCliKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCounts {
    pub counts: Vec<AgentSessionCount>,
    pub errors: std::collections::BTreeMap<AgentCliKind, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionPage {
    pub snapshot_id: String,
    pub scope_revision: String,
    pub items: Vec<AgentSessionRow>,
    pub parent_updates: Vec<AgentSessionParentUpdate>,
    pub next_cursor: Option<String>,
    pub loaded_count: usize,
    pub total: Option<usize>,
    pub agent_counts: Vec<AgentSessionCount>,
    pub source_states: Vec<AgentSessionSourceState>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDetailRequest {
    pub consumer_id: String,
    pub request_id: u64,
    pub scope_revision: String,
    pub session_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDetail {
    pub row: AgentSessionRow,
    pub detail: CliSessionDetail,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCancelRequest {
    pub consumer_id: String,
    pub request_id: u64,
}
