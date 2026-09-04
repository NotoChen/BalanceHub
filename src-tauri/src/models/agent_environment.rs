use super::AgentCliKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentEnvironmentKind {
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentHostPlatform {
    Macos,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentEnvironmentCapability {
    ReadOnlyInventory,
    BoundedPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEnvironmentDescriptor {
    pub id: String,
    pub kind: AgentEnvironmentKind,
    pub host_platform: AgentHostPlatform,
    pub guest_platform: Option<String>,
    pub display_name: String,
    pub capabilities: Vec<AgentEnvironmentCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetCategory {
    Config,
    Skill,
    Plugin,
    Extension,
    Mcp,
    Hook,
    StatusUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetScope {
    User,
    Workspace,
    Local,
    System,
    Managed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetState {
    Enabled,
    Disabled,
    Shadowed,
    Blocked,
    Invalid,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetMutation {
    ReadOnly,
    NativeToggle,
    ManagedMutation,
    ExternalCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetCapability {
    pub category: AgentAssetCategory,
    pub discovery: Vec<AgentAssetScope>,
    pub mutation: AgentAssetMutation,
    pub requires_restart: bool,
    pub requires_trust: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTrustState {
    Trusted,
    Untrusted,
    Required,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentInstallationChannel {
    Stable,
    Preview,
    Nightly,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentVersionState {
    UpToDate,
    UpdateAvailable,
    AheadOfStable,
    Unknown,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentVersionSource {
    NpmRegistry,
    LocalExecutable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentDiscoverySource {
    Configured,
    Automatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentInstallationAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetSource {
    pub id: String,
    pub scope: AgentAssetScope,
    pub environment_id: String,
    pub workspace_id: Option<String>,
    pub path: String,
    pub precedence: u32,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallation {
    pub id: String,
    pub environment_id: String,
    pub agent_kind: AgentCliKind,
    pub label: String,
    pub availability: AgentInstallationAvailability,
    pub executable_path: Option<String>,
    pub installed_version: Option<String>,
    pub discovery_source: AgentDiscoverySource,
    pub channel: AgentInstallationChannel,
    pub installed_version_source: AgentVersionSource,
    pub latest_stable_version: Option<String>,
    pub latest_version_source: AgentVersionSource,
    pub version_state: AgentVersionState,
    pub version_checked_at: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetOpenTarget {
    Asset,
    ParentDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetRecord {
    pub stable_id: String,
    pub agent_kind: AgentCliKind,
    pub category: AgentAssetCategory,
    pub native_id: String,
    pub label: String,
    pub source_id: String,
    pub scope: AgentAssetScope,
    pub environment_id: String,
    pub workspace_id: Option<String>,
    pub path: Option<String>,
    pub precedence: u32,
    pub writable: bool,
    pub declared_state: AgentAssetState,
    pub effective_state: AgentAssetState,
    pub trust_state: Option<AgentTrustState>,
    pub diagnostics: Vec<String>,
    pub revision: Option<String>,
    pub sensitive: bool,
    pub is_directory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEnvironmentInventory {
    pub environment: AgentEnvironmentDescriptor,
    pub installations: Vec<AgentInstallation>,
    pub sources: Vec<AgentAssetSource>,
    pub capabilities: Vec<AgentCapabilities>,
    pub assets: Vec<AgentAssetRecord>,
    pub scanned_at: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub agent_kind: AgentCliKind,
    pub assets: Vec<AgentAssetCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetReadResult {
    pub stable_id: String,
    pub path: String,
    pub content: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub truncated: bool,
    pub metadata_only: bool,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentVersionCheckResult {
    pub installations: Vec<AgentInstallation>,
    pub checked_at: String,
}
