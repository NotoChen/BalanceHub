use super::AgentAssetScope;
use serde::{Deserialize, Serialize};

/// How an observed asset is supplied, independent of scope and library ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetProvision {
    AgentBuiltIn,
    PluginProvided,
    Independent,
    Unknown,
}

/// Authorship requires evidence; a user-writable path does not prove authorship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetProviderOrigin {
    AgentVendor,
    ThirdParty,
    UserDeclared,
    Unknown,
}

/// Adapter-selected native entry point, rather than a guess from display paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetInstallationOrigin {
    Bundled,
    NativePackage,
    LocalFiles,
    SharedFiles,
    Linked,
    ConfigEntry,
    #[default]
    Unknown,
}

/// One contributing Definition's provenance. Merged records retain all such
/// evidence instead of selecting the first source or borrowing an overlay.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetProvenance {
    pub source_id: String,
    pub declaration_id: String,
    /// Native configuration scope, which can differ from the physical file's scope.
    pub scope: AgentAssetScope,
    pub provision: AgentAssetProvision,
    pub installation: AgentAssetInstallationOrigin,
    pub provider: AgentAssetProviderOrigin,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetProvenanceSummary {
    pub provisions: Vec<AgentAssetProvision>,
    pub installations: Vec<AgentAssetInstallationOrigin>,
    pub providers: Vec<AgentAssetProviderOrigin>,
}
