use crate::models::{
    AgentConfigurationDiagnostic, AgentConfigurationDiagnosticSeverity, AgentConfigurationError,
    AgentConfigurationErrorKind,
};

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
