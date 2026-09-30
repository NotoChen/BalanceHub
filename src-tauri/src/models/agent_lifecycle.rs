//! Window-bound upgrade contracts for existing Agent CLI installations.

use super::{
    AgentAssetOperationOutcome, AgentAssetOperationPhase, AgentCliKind, AgentInstallation,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleActionKind {
    Upgrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleChannel {
    Npm,
    HomebrewFormula,
    HomebrewCask,
    VendorNative,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleVersionSource {
    Homebrew,
    NpmRegistry,
    VendorRelease,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleVersionState {
    NotChecked,
    Unsupported,
    Unknown,
    CheckFailed,
    UpToDate,
    UpdateAvailable,
    AheadOfLatest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleUnavailableReason {
    RuntimeUnavailable,
    UnsupportedChannel,
    UnsupportedPlatform,
    InstallationUnavailable,
    ProvenanceUnverified,
    DirectoryConflict,
    PermissionRequired,
    VersionUnavailable,
    AlreadyCurrent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleAction {
    pub kind: AgentLifecycleActionKind,
    pub available: bool,
    pub reason: Option<AgentLifecycleUnavailableReason>,
    pub reason_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleVersion {
    pub state: AgentLifecycleVersionState,
    pub source: AgentLifecycleVersionSource,
    pub latest_version: Option<String>,
    pub checked_at: Option<String>,
    pub last_success_at: Option<String>,
    pub next_check_at: Option<String>,
    pub stale: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleTarget {
    pub id: String,
    pub agent_kind: AgentCliKind,
    pub label: String,
    pub installation: AgentInstallation,
    pub is_current: bool,
    pub channel: AgentLifecycleChannel,
    pub channel_label: String,
    pub release_track: Option<String>,
    pub directory: Option<String>,
    pub evidence_revision: String,
    pub version: AgentLifecycleVersion,
    pub actions: Vec<AgentLifecycleAction>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleVersionRefresh {
    #[default]
    Cached,
    IfStale,
    Force,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentLifecycleCatalogRequest {
    pub agent_kind: Option<AgentCliKind>,
    #[serde(default)]
    pub version_refresh: AgentLifecycleVersionRefresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleCatalog {
    pub targets: Vec<AgentLifecycleTarget>,
    pub refreshed_at: String,
    pub next_check_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentLifecyclePlanRequest {
    pub agent_kind: AgentCliKind,
    pub target_id: String,
    pub action: AgentLifecycleActionKind,
    pub expected_evidence_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecyclePlan {
    pub plan_token: String,
    pub agent_kind: AgentCliKind,
    pub target_id: String,
    pub installation_id: String,
    pub action: AgentLifecycleActionKind,
    pub channel: AgentLifecycleChannel,
    pub channel_label: String,
    pub directory: String,
    pub from_version: Option<String>,
    /// Release observed during planning; the channel updater chooses the installed version.
    pub to_version: String,
    pub mechanism_id: String,
    pub changes: Vec<String>,
    pub command_preview: Vec<String>,
    pub affected_installation_ids: Vec<String>,
    pub confirmation_message: String,
    pub cancellation_boundary: String,
    pub timeout_seconds: u64,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentLifecycleApplyRequest {
    pub plan_token: String,
    pub agent_kind: AgentCliKind,
    pub target_id: String,
    pub action: AgentLifecycleActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleNextLaunch {
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub uses_upgraded_installation: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleDiagnostics {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleOperation {
    pub id: String,
    pub agent_kind: AgentCliKind,
    pub target_id: String,
    pub installation_id: String,
    pub action: AgentLifecycleActionKind,
    pub channel: AgentLifecycleChannel,
    pub channel_label: String,
    pub directory: String,
    pub from_version: Option<String>,
    /// Release observed during planning; the channel updater chooses the installed version.
    pub to_version: String,
    pub observed_version: Option<String>,
    pub recovered: bool,
    pub next_launch: Option<AgentLifecycleNextLaunch>,
    pub verified_executable_path: Option<String>,
    pub phase: AgentAssetOperationPhase,
    pub can_cancel: bool,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub outcome: Option<AgentAssetOperationOutcome>,
    pub message: Option<String>,
    pub timed_out: bool,
    #[serde(default)]
    pub command_preview: Vec<String>,
    #[serde(default)]
    pub diagnostics: Option<AgentLifecycleDiagnostics>,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentLifecycleErrorKind {
    InvalidRequest,
    PlanExpired,
    PlanConsumed,
    ActorMismatch,
    TargetMismatch,
    ActionMismatch,
    TargetChanged,
    ActionUnavailable,
    VersionCheckFailed,
    OperationNotFound,
    CapacityExceeded,
    InternalFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLifecycleError {
    pub kind: AgentLifecycleErrorKind,
    pub message: String,
    pub reason: Option<AgentLifecycleUnavailableReason>,
}

impl AgentLifecycleError {
    pub fn new(kind: AgentLifecycleErrorKind) -> Self {
        use AgentLifecycleErrorKind::*;
        let message = match kind {
            InvalidRequest => "Agent 升级请求无效",
            PlanExpired => "升级计划已过期，请重新确认",
            PlanConsumed => "升级计划已使用，请重新生成计划",
            ActorMismatch => "升级计划不属于当前窗口",
            TargetMismatch => "升级目标与计划不一致",
            ActionMismatch => "升级动作与计划不一致",
            TargetChanged => "选中安装或运行环境已经变化，请刷新后重新确认",
            ActionUnavailable => "当前安装没有可验证的升级方式",
            VersionCheckFailed => "无法确认该安装渠道的最新版本，请稍后重试",
            OperationNotFound => "未找到当前窗口的升级任务",
            CapacityExceeded => "升级任务数量已达上限，请等待已有任务完成",
            InternalFailure => "Agent 升级服务暂时不可用",
        };
        Self {
            kind,
            message: message.to_owned(),
            reason: None,
        }
    }
}

impl std::fmt::Display for AgentLifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentLifecycleError {}
