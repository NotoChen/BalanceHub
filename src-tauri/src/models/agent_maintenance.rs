use super::{AgentAssetOperationPhase, AgentAssetPlanChange, AgentCliKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHistoryEntry {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub file_count: usize,
    pub bytes: u64,
    pub paths: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMaintenanceKind {
    Restore,
    SharedVersion,
    Batch,
    SkillUpdate,
    McpTools,
    SessionRelocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMaintenanceItemState {
    Pending,
    Running,
    Completed,
    Skipped,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMaintenanceOutcome {
    Succeeded,
    Partial,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMaintenanceItem {
    pub id: String,
    pub label: String,
    pub state: AgentMaintenanceItemState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMaintenanceRefresh {
    pub catalog: bool,
    pub agents: Vec<AgentCliKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMaintenancePlan {
    pub token: String,
    pub title: String,
    pub kind: AgentMaintenanceKind,
    pub expires_at: String,
    pub changes: Vec<AgentAssetPlanChange>,
    pub items: Vec<AgentMaintenanceItem>,
    pub notes: Vec<String>,
    pub refresh: AgentMaintenanceRefresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMaintenanceOperation {
    pub id: String,
    pub plan_token: String,
    pub kind: AgentMaintenanceKind,
    pub title: String,
    pub revision: u64,
    pub phase: AgentAssetOperationPhase,
    pub outcome: Option<AgentMaintenanceOutcome>,
    pub can_cancel: bool,
    pub created_at: String,
    pub updated_at: String,
    pub message: String,
    pub completed: usize,
    pub total: usize,
    pub items: Vec<AgentMaintenanceItem>,
    pub refresh: AgentMaintenanceRefresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMaintenanceApplyRequest {
    pub plan_token: String,
    pub kind: AgentMaintenanceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSharedVersion {
    pub version: u64,
    pub current: bool,
    pub bytes: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillOrigin {
    pub repository: String,
    pub reference: String,
    pub directory: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillUpdateCheck {
    pub revision: String,
    pub changed: bool,
    pub checked_at: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBatchOption {
    pub asset_id: String,
    pub name: String,
    pub request: super::AgentCatalogPlanRequest,
    pub plan: Option<super::AgentCatalogPlan>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpToolsRequest {
    pub asset_id: String,
    pub binding_id: String,
    pub workspace: Option<String>,
    pub tools: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpToolRule {
    pub name: String,
    pub disabled_here: bool,
    pub restricted_by: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpToolRules {
    pub binding_id: String,
    pub rules: Vec<AgentMcpToolRule>,
    pub path: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRelocationRequest {
    pub scope_revision: String,
    pub session_ref: String,
    pub destination: String,
    pub consumer_id: String,
    pub request_id: u64,
}
