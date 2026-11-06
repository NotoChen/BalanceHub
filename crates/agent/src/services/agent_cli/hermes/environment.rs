use super::config;
use crate::models::AgentConfigurationContext;
use crate::services::agent_cli::contracts::{AgentContextDiscoveryRequest, AgentDiagnosticOutput};

/// Config-only surface: bump when the native settings shape starts feeding a
/// parser. Asset discovery is intentionally empty for this integration tier.
pub(super) const PARSER_VERSION: u32 = 1;

pub(super) fn discover_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    crate::services::agent_cli::integration::contexts(
        request,
        config::active_dir_for_home(request.home),
        PARSER_VERSION,
    )
}
