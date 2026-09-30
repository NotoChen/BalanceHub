//! Public contracts for verified asset access and confirmed native operations.
//! These are transient IPC facts, never another configuration store.

use super::{
    AgentAssetActionKind, AgentAssetActionUnavailableReason, AgentAssetOpenTarget,
    AgentHostPlatform,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentHostArchitecture {
    Aarch64,
    X86_64,
    Unknown,
}

impl AgentHostArchitecture {
    pub fn current() -> Self {
        match std::env::consts::ARCH {
            "aarch64" => Self::Aarch64,
            "x86_64" => Self::X86_64,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNativeTarget {
    pub platform: AgentHostPlatform,
    pub architecture: AgentHostArchitecture,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCliDistribution {
    Npm,
    Homebrew,
    VendorNative,
    WinGet,
    Apt,
    Dnf,
    Apk,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AgentVersionIdentifier {
    Numeric(u64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSemanticVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Vec<AgentVersionIdentifier>,
}

impl Ord for AgentSemanticVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(
                || match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => self.prerelease.cmp(&other.prerelease),
                },
            )
    }
}

impl PartialOrd for AgentSemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentMechanismId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetAccessUnavailableReason {
    SourceUnavailable,
    SnapshotUnavailable,
    PolicyUnavailable,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentAssetAccess {
    Unavailable {
        reason: AgentAssetAccessUnavailableReason,
    },
    Ready {
        access_id: String,
    },
}

impl Default for AgentAssetAccess {
    fn default() -> Self {
        Self::Unavailable {
            reason: AgentAssetAccessUnavailableReason::SnapshotUnavailable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetAccessRisk {
    ExternalPathnameRace,
    RawSensitiveContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetAccessErrorKind {
    AccessExpired,
    ActorMismatch,
    TargetMismatch,
    EnvironmentMismatch,
    WorkspaceMismatch,
    RootChanged,
    SourceChanged,
    SchemaChanged,
    PolicyUnavailable,
    OutsideAllowedRoot,
    SymlinkRejected,
    ConfirmationRequired,
    ExternalOpenFailed,
    UnsupportedPlatform,
    AccessUnavailable,
    ReadFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetAccessError {
    pub kind: AgentAssetAccessErrorKind,
    pub message: String,
}

impl AgentAssetAccessError {
    pub fn new(kind: AgentAssetAccessErrorKind) -> Self {
        use AgentAssetAccessErrorKind::*;
        let message = match kind {
            AccessExpired => "资产访问已过期，请刷新列表后重试",
            ActorMismatch => "该访问不属于当前窗口",
            TargetMismatch => "资产访问目标不匹配",
            EnvironmentMismatch => "资产环境已变化，请刷新列表",
            WorkspaceMismatch => "工作区已变化，请刷新列表",
            RootChanged => "配置目录对象已变化，请刷新列表",
            SourceChanged => "配置文件已变化，请刷新列表",
            SchemaChanged => "配置解析规则已变化，请刷新列表",
            PolicyUnavailable => "该配置暂不支持安全预览",
            OutsideAllowedRoot => "配置来源不在允许的目录范围内",
            SymlinkRejected => "配置路径包含链接，无法安全访问",
            ConfirmationRequired => "请先确认外部打开风险",
            ExternalOpenFailed => "系统未能打开该配置来源",
            UnsupportedPlatform => "当前平台暂不支持此安全访问方式",
            AccessUnavailable => "当前来源不可访问，请刷新列表",
            ReadFailed => "无法读取当前配置来源",
        };
        Self {
            kind,
            message: message.to_owned(),
        }
    }
}

impl std::fmt::Display for AgentAssetAccessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentAssetAccessError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAssetReadRequest {
    pub target_id: String,
    pub access_id: String,
    pub environment_id: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAssetOpenRequest {
    pub target_id: String,
    pub access_id: String,
    pub environment_id: String,
    pub workspace: Option<String>,
    pub target: AgentAssetOpenTarget,
    pub accepted_risks: Vec<AgentAssetAccessRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAssetPlanRequest {
    pub asset_id: String,
    pub action: AgentAssetActionKind,
    pub workspace: Option<String>,
    pub expected_revision: String,
    pub installation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAssetApplyRequest {
    pub plan_token: String,
    pub asset_id: String,
    pub action: AgentAssetActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetPlanChange {
    pub label: String,
    pub path: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetPlan {
    pub token: String,
    pub asset_id: String,
    pub action: AgentAssetActionKind,
    pub title: String,
    pub mechanism_id: String,
    pub selected_installation_id: Option<String>,
    pub expires_at: String,
    pub changes: Vec<AgentAssetPlanChange>,
    pub affected_asset_ids: Vec<String>,
    pub affected_installation_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub reload_effect: Option<String>,
    pub trust_effect: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetOperationPhase {
    Preparing,
    WaitingForLock,
    Revalidating,
    Applying,
    Verifying,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetOperationOutcome {
    CanceledBeforeCommit,
    UnchangedConflict,
    UnchangedFailure,
    AppliedVerified,
    AppliedUnverified,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetOperation {
    pub id: String,
    pub asset_id: String,
    pub action: AgentAssetActionKind,
    pub phase: AgentAssetOperationPhase,
    pub can_cancel: bool,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub outcome: Option<AgentAssetOperationOutcome>,
    pub message: Option<String>,
    pub affected_asset_ids: Vec<String>,
    pub reload_effect: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetMutationErrorKind {
    InvalidRequest,
    PlanExpired,
    PlanConsumed,
    ActorMismatch,
    TargetMismatch,
    ActionMismatch,
    OperationNotFound,
    SourceConflict,
    ActionUnavailable,
    PreparationFailed,
    CapacityExceeded,
    InternalFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetMutationError {
    pub kind: AgentAssetMutationErrorKind,
    pub message: String,
    pub reason: Option<AgentAssetActionUnavailableReason>,
}

impl AgentAssetMutationError {
    pub fn new(kind: AgentAssetMutationErrorKind) -> Self {
        use AgentAssetMutationErrorKind::*;
        let message = match kind {
            InvalidRequest => "操作请求无效",
            PlanExpired => "操作计划已过期，请重新确认",
            PlanConsumed => "操作计划已使用，请重新生成计划",
            ActorMismatch => "该操作不属于当前窗口",
            TargetMismatch => "操作目标与计划不一致",
            ActionMismatch => "操作动作与计划不一致",
            OperationNotFound => "未找到该后台操作",
            SourceConflict => "配置在确认后已变化，未执行修改",
            ActionUnavailable => "当前资产不支持该操作",
            PreparationFailed => "无法准备该操作，请刷新资产状态",
            CapacityExceeded => "当前后台操作过多，请稍后重试",
            InternalFailure => "后台操作无法完成，请查看并刷新资产状态",
        };
        Self {
            kind,
            message: message.to_owned(),
            reason: None,
        }
    }

    pub fn unavailable(reason: AgentAssetActionUnavailableReason) -> Self {
        Self {
            reason: Some(reason),
            ..Self::new(AgentAssetMutationErrorKind::ActionUnavailable)
        }
    }
}

impl std::fmt::Display for AgentAssetMutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentAssetMutationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn access_and_native_target_use_the_frontend_wire_names() {
        assert_eq!(
            serde_json::to_value(AgentAssetAccess::Ready {
                access_id: "opaque".into()
            })
            .unwrap(),
            json!({"kind":"ready","accessId":"opaque"})
        );
        assert_eq!(
            serde_json::to_value(AgentHostArchitecture::X86_64).unwrap(),
            json!("x86_64")
        );
    }

    #[test]
    fn apply_request_cannot_smuggle_client_write_authority() {
        let valid = json!({"planToken":"opaque", "assetId":"asset", "action":"disable"});
        assert!(serde_json::from_value::<AgentAssetApplyRequest>(valid.clone()).is_ok());
        for field in ["path", "changes", "argv", "actor", "mechanismId"] {
            let mut invalid = valid.clone();
            invalid[field] = json!("untrusted");
            assert!(
                serde_json::from_value::<AgentAssetApplyRequest>(invalid).is_err(),
                "{field}"
            );
        }
        assert!(serde_json::from_value::<AgentAssetApplyRequest>(
            json!({"assetId":"asset","action":"disable"})
        )
        .is_err());
    }
}
