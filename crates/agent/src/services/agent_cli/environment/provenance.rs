//! Project explicit native source facts after definition and relationship
//! validation. Paths, scope, labels and library ownership never prove an author.
use crate::models::{
    AgentAssetInstallationOrigin, AgentAssetProvenance, AgentAssetProviderOrigin,
    AgentAssetProvision, AgentAssetScope,
};

pub(super) fn definition_provenance(
    source_id: String,
    declaration_id: String,
    scope: AgentAssetScope,
    installation: AgentAssetInstallationOrigin,
    provider: AgentAssetProviderOrigin,
    declares_provider: bool,
    provider_resolved: bool,
) -> AgentAssetProvenance {
    let provision = if provider_resolved {
        AgentAssetProvision::PluginProvided
    } else if declares_provider {
        AgentAssetProvision::Unknown
    } else {
        match installation {
            AgentAssetInstallationOrigin::Bundled => AgentAssetProvision::AgentBuiltIn,
            AgentAssetInstallationOrigin::Unknown => AgentAssetProvision::Unknown,
            _ => AgentAssetProvision::Independent,
        }
    };
    AgentAssetProvenance {
        source_id,
        declaration_id,
        scope,
        provision,
        installation,
        provider,
    }
}
