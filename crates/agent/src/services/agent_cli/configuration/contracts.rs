//! Native configuration facts. None of these types is an IPC authority.
use crate::models::{
    AgentConfigurationDiagnostic, AgentConfigurationError, AgentConfigurationFormat,
};
use crate::services::agent_cli::contracts::{AgentAssetSourceSpec, AgentSourceDiscoveryRequest};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct AgentConfigurationSourceSpec {
    pub native: AgentAssetSourceSpec,
    pub format: AgentConfigurationFormat,
    pub may_create: bool,
    pub profile: Option<String>,
    pub load_rule: String,
    pub reload_hint: String,
    pub version_requirement: Option<String>,
    pub initial_text: Option<String>,
}
pub(crate) struct ConfigurationValidationRequest<'a> {
    pub source: &'a AgentConfigurationSourceSpec,
    pub after: &'a str,
}
pub(crate) struct ConfigurationValidation {
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
    pub reload_hints: Vec<String>,
}
pub(crate) trait AgentConfigurationSourceOutput {
    fn emit(&mut self, source: AgentConfigurationSourceSpec) -> std::ops::ControlFlow<()>;
    fn diagnostic(&mut self, diagnostic: AgentConfigurationDiagnostic);
}
#[derive(Clone, Copy)]
pub(crate) struct AgentConfigurationAdapter {
    pub discover:
        for<'a> fn(AgentSourceDiscoveryRequest<'a>, &mut dyn AgentConfigurationSourceOutput),
    pub validate: for<'a> fn(
        ConfigurationValidationRequest<'a>,
    ) -> Result<ConfigurationValidation, AgentConfigurationError>,
}

/// Provider merge output remains in backend memory. Paths must match a source
/// issued by the native configuration adapter before an edit is admitted.
/// Intentionally neither Serialize nor Debug: both strings may hold secrets.
pub(crate) struct ProviderConfigurationCandidate {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
}
