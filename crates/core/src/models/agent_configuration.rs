//! IPC shapes for native Agent configuration management.
//! Local configuration previews and edits carry the original file text.
use super::{
    AgentAssetAccess, AgentAssetAccessError, AgentAssetAccessErrorKind, AgentAssetAccessRisk,
    AgentAssetCategory, AgentAssetOpenTarget, AgentAssetOperationOutcome, AgentAssetOperationPhase,
    AgentAssetPlanChange, AgentAssetRevision, AgentAssetScope, AgentCliKind,
};
use serde::{Deserialize, Serialize};
use std::fmt;

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
pub enum AgentConfigurationContentState {
    Unknown,
    Empty,
    NonEmpty,
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
    #[serde(default)]
    pub runtime_scope: super::AgentRuntimeScope,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationSource {
    pub visual_settings: bool,
    pub source_id: String,
    pub context_id: String,
    pub environment_id: String,
    pub runtime_scope: super::AgentRuntimeScope,
    pub agent_kind: AgentCliKind,
    pub workspace: Option<String>,
    pub native_role: String,
    pub scope: AgentAssetScope,
    pub profile: Option<String>,
    pub label: String,
    #[serde(serialize_with = "crate::platform::paths::serialize_path")]
    pub path: String,
    pub format: AgentConfigurationFormat,
    pub content_state: AgentConfigurationContentState,
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
    pub visual_settings: bool,
    pub source_id: String,
    pub label: String,
    #[serde(serialize_with = "crate::platform::paths::serialize_path")]
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

impl AgentConfigurationError {
    pub fn new(kind: AgentConfigurationErrorKind) -> Self {
        use AgentConfigurationErrorKind as K;
        let message = match kind {
            K::InvalidRequest => "配置请求无效，请重新打开配置面板",
            K::AccessExpired => "配置来源访问已过期，请刷新来源列表",
            K::ActorMismatch => "配置编辑不属于当前窗口",
            K::SourceChanged => "配置文件已被其他操作修改，请重新读取后比较草稿",
            K::RootChanged => "配置目录已发生变化，请刷新来源列表",
            K::SourceUnavailable => "无法读取配置来源，请检查文件和目录权限",
            K::ReadOnly => "文件或所在目录不可写，请检查当前用户的文件权限",
            K::UnsupportedFormat => "此来源不支持文本编辑",
            K::InvalidSyntax => "配置语法无效，请修正草稿后重试",
            K::UnsupportedScope => "此修改不适用于所选来源，请使用对应资源或原生管理入口",
            K::EditExpired => "配置草稿已过期，请重新读取配置",
            K::PlanExpired => "保存计划已过期，请重新生成计划",
            K::PlanConsumed => "保存计划已提交，不能再次执行",
            K::OperationNotFound => "配置后台操作不存在或已清理",
            K::CapacityExceeded => "配置编辑超出数量或大小限制，请关闭其他草稿后重试",
            K::Timeout => "配置操作超过时间限制，请检查后台结果后重试",
            K::Canceled => "配置操作已取消",
            K::WriteFailed => "配置写入未完成，请检查每个文件的结果",
            K::UnsupportedPlatform => "此来源在当前平台无法安全写入",
            K::InternalFailure => "配置操作未完成，请重新打开面板",
        };
        Self {
            kind,
            message: message.to_owned(),
            diagnostics: Vec::new(),
        }
    }
}

impl fmt::Display for AgentConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for AgentConfigurationError {}

impl From<AgentAssetAccessError> for AgentConfigurationError {
    fn from(error: AgentAssetAccessError) -> Self {
        use AgentAssetAccessErrorKind as A;
        use AgentConfigurationErrorKind as C;
        Self::new(match error.kind {
            A::ActorMismatch => C::ActorMismatch,
            A::AccessExpired => C::AccessExpired,
            A::RootChanged => C::RootChanged,
            A::SourceChanged | A::SchemaChanged => C::SourceChanged,
            A::TargetMismatch
            | A::EnvironmentMismatch
            | A::WorkspaceMismatch
            | A::ConfirmationRequired => C::InvalidRequest,
            _ => C::SourceUnavailable,
        })
    }
}

impl From<crate::models::AgentAssetMutationError> for AgentConfigurationError {
    fn from(error: crate::models::AgentAssetMutationError) -> Self {
        use crate::models::AgentAssetMutationErrorKind as M;
        Self::new(match error.kind {
            M::SourceConflict => AgentConfigurationErrorKind::SourceChanged,
            M::ActionUnavailable => AgentConfigurationErrorKind::SourceUnavailable,
            M::InternalFailure => AgentConfigurationErrorKind::InternalFailure,
            _ => AgentConfigurationErrorKind::WriteFailed,
        })
    }
}
