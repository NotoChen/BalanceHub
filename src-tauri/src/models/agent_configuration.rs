//! IPC shapes for native Agent configuration management.
//! Local configuration previews and edits carry the original file text.
use super::{
    AgentAssetAccess, AgentAssetAccessRisk, AgentAssetCategory, AgentAssetOpenTarget,
    AgentAssetOperationOutcome, AgentAssetOperationPhase, AgentAssetPlanChange, AgentAssetRevision,
    AgentAssetScope, AgentCliKind,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigurationFormat {
    Toml,
    Json,
    Jsonc,
    Dotenv,
    Markdown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigurationActionKind {
    Read,
    Edit,
    Create,
    Open,
    Reveal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigurationEffectKind {
    Contributes,
    Overridden,
    Inactive,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigurationDiagnosticSeverity {
    Info,
    Warning,
    Error,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigurationFileState {
    Unchanged,
    Applied,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConfigurationErrorKind {
    InvalidRequest,
    AccessExpired,
    ActorMismatch,
    SourceChanged,
    RootChanged,
    SourceUnavailable,
    ReadOnly,
    UnsupportedFormat,
    InvalidSyntax,
    UnsupportedScope,
    EditExpired,
    PlanExpired,
    PlanConsumed,
    OperationNotFound,
    CapacityExceeded,
    Timeout,
    Canceled,
    WriteFailed,
    UnsupportedPlatform,
    InternalFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationDiagnostic {
    pub code: String,
    pub severity: AgentConfigurationDiagnosticSeverity,
    pub message: String,
    pub source_id: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationAction {
    pub action: AgentConfigurationActionKind,
    pub available: bool,
    pub reason: Option<String>,
    pub risks: Vec<AgentAssetAccessRisk>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationEffect {
    pub kind: AgentConfigurationEffectKind,
    pub message: String,
    pub related_source_ids: Vec<String>,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationListRequest {
    pub agent_kind: AgentCliKind,
    pub workspace: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationSource {
    pub source_id: String,
    pub context_id: String,
    pub environment_id: String,
    pub agent_kind: AgentCliKind,
    pub workspace: Option<String>,
    pub native_role: String,
    pub scope: AgentAssetScope,
    pub profile: Option<String>,
    pub label: String,
    pub path: String,
    pub format: AgentConfigurationFormat,
    pub revision: AgentAssetRevision,
    pub access: AgentAssetAccess,
    pub actions: Vec<AgentConfigurationAction>,
    pub effect: AgentConfigurationEffect,
    pub reload_hint: String,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationSnapshot {
    pub revision: String,
    pub agent_kind: AgentCliKind,
    pub environment_id: String,
    pub workspace: Option<String>,
    pub sources: Vec<AgentConfigurationSource>,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationSourceRequest {
    pub source_id: String,
    pub access_id: String,
    pub environment_id: String,
    pub workspace: Option<String>,
    pub expected_revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationOpenRequest {
    pub source_id: String,
    pub access_id: String,
    pub environment_id: String,
    pub workspace: Option<String>,
    pub expected_revision: String,
    pub target: AgentAssetOpenTarget,
    pub accepted_risks: Vec<AgentAssetAccessRisk>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationReadResult {
    pub source_id: String,
    pub revision: String,
    pub text: String,
    pub truncated: bool,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationEditableDocument {
    pub source_id: String,
    pub label: String,
    pub path: String,
    pub format: AgentConfigurationFormat,
    pub creating: bool,
    pub original_text: String,
    pub text: String,
    pub read_only_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentResourceEditRequest {
    pub asset_id: String,
    pub workspace: Option<String>,
    pub document_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResourceContentFact {
    pub label: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResourceContent {
    pub asset_id: String,
    pub category: AgentAssetCategory,
    pub description: Option<String>,
    pub facts: Vec<AgentResourceContentFact>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationEdit {
    pub edit_id: String,
    pub revision: String,
    pub expires_at: String,
    pub agent_kind: AgentCliKind,
    pub documents: Vec<AgentConfigurationEditableDocument>,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
    pub resource: Option<AgentResourceContent>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationTextEdit {
    pub source_id: String,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationSaveRequest {
    pub edit_id: String,
    pub expected_revision: String,
    pub documents: Vec<AgentConfigurationTextEdit>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationPlan {
    pub token: String,
    pub edit_id: String,
    pub expires_at: String,
    pub source_ids: Vec<String>,
    pub changes: Vec<AgentAssetPlanChange>,
    pub reload_hints: Vec<String>,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfigurationApplyRequest {
    pub edit_id: String,
    pub plan_token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationFileResult {
    pub source_id: String,
    pub state: AgentConfigurationFileState,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationOperation {
    pub id: String,
    pub edit_id: String,
    pub agent_kind: AgentCliKind,
    pub source_ids: Vec<String>,
    pub revision: u64,
    pub phase: AgentAssetOperationPhase,
    pub outcome: Option<AgentAssetOperationOutcome>,
    pub can_cancel: bool,
    pub created_at: String,
    pub updated_at: String,
    pub message: Option<String>,
    pub files: Vec<AgentConfigurationFileResult>,
    pub reload_hints: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationError {
    pub kind: AgentConfigurationErrorKind,
    pub message: String,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
}
