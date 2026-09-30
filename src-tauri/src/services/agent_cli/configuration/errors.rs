use crate::models::{
    AgentAssetAccessError, AgentAssetAccessErrorKind, AgentConfigurationDiagnostic,
    AgentConfigurationDiagnosticSeverity, AgentConfigurationError, AgentConfigurationErrorKind,
};
use std::fmt;

impl AgentConfigurationError {
    pub(crate) fn new(kind: AgentConfigurationErrorKind) -> Self {
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

pub(super) fn diagnostic(
    code: &str,
    message: &str,
    severity: AgentConfigurationDiagnosticSeverity,
) -> AgentConfigurationDiagnostic {
    AgentConfigurationDiagnostic {
        code: code.to_owned(),
        severity,
        message: message.to_owned(),
        source_id: None,
        line: None,
        column: None,
    }
}
pub(super) fn internal() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::InternalFailure)
}
pub(super) fn conflict() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::SourceChanged)
}
