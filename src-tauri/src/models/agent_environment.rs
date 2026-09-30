use super::{
    AgentAssetAccess, AgentAssetAccessRisk, AgentAssetInstallationOrigin, AgentAssetProvenance,
    AgentCliDistribution, AgentCliKind, AgentHostArchitecture, AgentMechanismId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub host_architecture: AgentHostArchitecture,
    pub guest_platform: Option<String>,
    pub display_name: String,
    pub capabilities: Vec<AgentEnvironmentCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetCategory {
    Skill,
    Plugin,
    Extension,
    Mcp,
    Hook,
    StatusUi,
}

impl AgentAssetCategory {
    pub fn key(self) -> String {
        match self {
            Self::Skill => "skill",
            Self::Plugin => "plugin",
            Self::Extension => "extension",
            Self::Mcp => "mcp",
            Self::Hook => "hook",
            Self::StatusUi => "statusUi",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    NotInstalled,
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
pub enum AgentVersionSource {
    NpmRegistry,
    LocalExecutable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetLimitKind {
    CandidatePathsPerAgent,
    InstallationsPerAgent,
    SourcesPerContext,
    FirstLevelEntries,
    FrontmatterLines,
    FrontmatterBytes,
    BytesPerSource,
    BytesPerRefresh,
    RefreshBudget,
    CliOutput,
    CliConcurrency,
    Watchers,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetDocumentFormat {
    Json,
    Toml,
    Yaml,
    Manifest,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetSourceKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetIoErrorKind {
    NotFound,
    PermissionDenied,
    InvalidData,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentExecutableProbeErrorKind {
    NotFound,
    PermissionDenied,
    TimedOut,
    InvalidVersion,
    ChangedDuringProbe,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetRelationKind {
    ProvidedBy,
    ActionOwner,
    ExplicitImpact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetDeclarationRole {
    Definition,
    StateOverlay,
    PolicyOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetSuppressionReason {
    UntrustedWorkspace,
    CompatibilitySourceDisabled,
    UnsupportedContext,
    DuplicatePhysicalSource,
    ParentNotSelected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentAssetResolutionParticipation {
    Participates,
    Suppressed { reason: AgentAssetSuppressionReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetDiscoveryIncompleteReason {
    UnsupportedVersion,
    InstallationUnverified,
    SourceUnavailable,
    RuntimeStateUnobserved,
    UnsupportedEntryPoint,
    NativeEquivalenceUnobserved,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentAssetDiagnostic {
    Truncated {
        limit: AgentAssetLimitKind,
        accepted: u64,
        observed_at_least: u64,
    },
    Malformed {
        format: AgentAssetDocumentFormat,
        location: Option<String>,
    },
    DuplicateNativeId {
        category: AgentAssetCategory,
        native_id: String,
    },
    UnknownField {
        field_path: String,
    },
    SymlinkRejected {
        source_id: String,
    },
    BudgetExceeded {
        elapsed_ms: u64,
        budget_ms: u64,
    },
    ReadFailed {
        source_id: String,
        error_kind: AgentAssetIoErrorKind,
    },
    InvalidNativeId {
        category: AgentAssetCategory,
    },
    UnresolvedRelationship {
        relation: AgentAssetRelationKind,
        native_id: String,
    },
    InvalidProjection {
        projection_key: String,
    },
    InvalidResolution {
        projection_key: String,
        resolution: AgentAssetResolutionRelation,
    },
    InstallationProbeFailed {
        candidate_source: AgentDiscoverySource,
        error_kind: AgentExecutableProbeErrorKind,
    },
    SourceOutsideAllowedRoot {
        source_id: String,
    },
    SourceTypeMismatch {
        source_id: String,
        expected: AgentAssetSourceKind,
        actual: AgentAssetSourceKind,
    },
    InvalidCompatibleInstallation {
        installation_id: String,
    },
    DeclarationSuppressed {
        reason: AgentAssetSuppressionReason,
    },
    DiscoveryIncomplete {
        agent_kind: AgentCliKind,
        category: AgentAssetCategory,
        reason: AgentAssetDiscoveryIncompleteReason,
    },
    PolicyBlocked,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetRevision {
    pub identity: String,
    pub observed_at: String,
    pub size_bytes: Option<u64>,
    pub is_missing: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutableIdentity {
    pub owner: String,
    pub canonical_path: String,
    pub installation_source: AgentDiscoverySource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigurationContext {
    pub id: String,
    pub environment_id: String,
    pub agent_kind: AgentCliKind,
    pub config_root: String,
    pub profile: String,
    pub workspace_id: Option<String>,
    pub trust_context: AgentTrustState,
    pub parser_version: u32,
    pub schema_facts: BTreeMap<String, String>,
    pub compatible_installation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetPresence {
    Present,
    Missing,
    Invalid,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetDeclaredState {
    Enabled,
    Disabled,
    Pending,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetInstallState {
    Installed,
    NotInstalled,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetEvidence {
    pub revision: AgentAssetRevision,
    pub parser_version: u32,
    pub observed_at: String,
    pub facts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetDeclaration {
    pub id: String,
    pub context_id: String,
    pub source_id: String,
    pub scope: AgentAssetScope,
    pub native_kind: AgentAssetCategory,
    pub native_id: String,
    pub declaration_key: String,
    pub label: String,
    pub precedence: u32,
    pub presence: AgentAssetPresence,
    pub declared_state: AgentAssetDeclaredState,
    pub trust_state: AgentTrustState,
    pub role: AgentAssetDeclarationRole,
    pub participation: AgentAssetResolutionParticipation,
    pub evidence: AgentAssetEvidence,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
    pub provided_by: Option<AgentAssetNativeRef>,
    pub action_owner: Option<AgentAssetNativeRef>,
    pub explicitly_affected: Vec<AgentAssetNativeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetResolutionRelation {
    Independent,
    ReplaceWinner,
    Replaced,
    Merged,
    Additive,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetResolutionTerminal {
    PolicyBlocked,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetResolution {
    pub relation: AgentAssetResolutionRelation,
    pub qualified_collision: bool,
    pub terminal: Option<AgentAssetResolutionTerminal>,
    pub contributor_ids: Vec<String>,
    pub winner_id: Option<String>,
    pub control_source: Option<AgentAssetPolicyReference>,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentAssetPolicyReference {
    Declaration { declaration_id: String },
    Source { source_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetNativeRef {
    pub category: AgentAssetCategory,
    pub native_id: String,
    pub qualifier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetRelationships {
    pub provided_by: Option<String>,
    pub action_owner: Option<String>,
    pub affected_asset_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMcpTransport {
    Stdio,
    #[serde(alias = "streamable-http")]
    Http,
    Sse,
    #[serde(alias = "ws", alias = "websocket")]
    WebSocket,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMcpApprovalState {
    Approved,
    Rejected,
    Pending,
    NotRequired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetEffectiveAvailability {
    Available,
    Disabled,
    ApprovalRequired,
    PolicyBlocked,
    TrustRequired,
    Invalid,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSkillInvocationPolicy {
    ModelInvocable,
    ManualOnly,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatusUiMode {
    BuiltIn,
    Command,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentAssetDetails {
    Skill {
        enabled: AgentAssetDeclaredState,
        invocation_policy: AgentSkillInvocationPolicy,
    },
    Mcp {
        transport: AgentMcpTransport,
        declared_state: AgentAssetDeclaredState,
        approval_state: AgentMcpApprovalState,
        effective_availability: AgentAssetEffectiveAvailability,
    },
    Plugin {
        install_state: AgentAssetInstallState,
        enabled: AgentAssetDeclaredState,
        trusted: AgentTrustState,
    },
    Extension {
        install_state: AgentAssetInstallState,
        enabled: AgentAssetDeclaredState,
        trusted: AgentTrustState,
    },
    Hook {
        managed: bool,
        enabled: AgentAssetDeclaredState,
        /// Configured handler definitions, independent of enablement or trust.
        /// None means the native schema or source was not completely observed.
        rule_count: Option<u32>,
    },
    StatusUi {
        mode: AgentStatusUiMode,
        command_present: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetActionKind {
    Inspect,
    Enable,
    Disable,
    Preview,
    Remove,
    Open,
    Reveal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetActionUnavailableReason {
    MutationDisabled,
    NoOfficialMechanism,
    UnsupportedPlatform,
    UnsupportedScope,
    UnsupportedSchema,
    NoCompatibleInstallation,
    InstallationUnavailable,
    AssetNotInstalled,
    AssetInstallationUnknown,
    AmbiguousMechanism,
    NativeInteractiveOnly,
    InvocationPolicyOnly,
    NoReversibleMechanism,
    ManagedByAssetCatalog,
    TrustRequired,
    ScopeAmbiguous,
    ChildOwnedByParent,
    Shadowed,
    PolicyBlocked,
    SourceUnavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetAction {
    pub action: AgentAssetActionKind,
    pub available: bool,
    pub reason: Option<AgentAssetActionUnavailableReason>,
    pub mechanism_id: Option<String>,
    pub confirmation_required: bool,
    pub reload_effect: Option<String>,
    pub trust_effect: Option<String>,
    pub selected_installation_id: Option<String>,
    pub risks: Vec<AgentAssetAccessRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetMechanismRecord {
    pub id: AgentMechanismId,
    pub agent_kind: AgentCliKind,
    pub category: AgentAssetCategory,
    pub action: AgentAssetActionKind,
    pub platforms: Vec<AgentHostPlatform>,
    pub adapter_schema_version: u32,
    pub source_schema: Option<String>,
    pub executable_argv: Vec<String>,
    pub scopes: Vec<AgentAssetScope>,
    pub inspection: String,
    pub idempotent: bool,
    pub commit_point: String,
    pub reload_effect: Option<String>,
    pub redaction_rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetLimits {
    pub candidate_paths_per_agent: usize,
    pub installations_per_agent: usize,
    pub sources_per_context: usize,
    pub first_level_entries: usize,
    pub bytes_per_source: usize,
    pub bytes_per_refresh: usize,
    pub refresh_budget_ms: u64,
    pub cli_output_bytes: usize,
    pub cli_concurrency: usize,
    pub watchers: usize,
    pub diagnostics: usize,
}

impl AgentAssetLimits {
    pub const DEFAULT: Self = Self {
        candidate_paths_per_agent: 32,
        installations_per_agent: 8,
        sources_per_context: 128,
        first_level_entries: 512,
        bytes_per_source: 512 * 1024,
        bytes_per_refresh: 8 * 1024 * 1024,
        refresh_budget_ms: 5_000,
        cli_output_bytes: 256 * 1024,
        cli_concurrency: 2,
        watchers: 64,
        diagnostics: 100,
    };

    pub const HARD_CAP: Self = Self {
        candidate_paths_per_agent: 128,
        installations_per_agent: 32,
        sources_per_context: 512,
        first_level_entries: 2_048,
        bytes_per_source: 2 * 1024 * 1024,
        bytes_per_refresh: 32 * 1024 * 1024,
        refresh_budget_ms: 15_000,
        cli_output_bytes: 1024 * 1024,
        cli_concurrency: 4,
        watchers: 256,
        diagnostics: 500,
    };

    pub const fn defaults() -> Self {
        Self::DEFAULT
    }

    pub fn bounded(self) -> Self {
        Self {
            candidate_paths_per_agent: self
                .candidate_paths_per_agent
                .min(Self::HARD_CAP.candidate_paths_per_agent),
            installations_per_agent: self
                .installations_per_agent
                .min(Self::HARD_CAP.installations_per_agent),
            sources_per_context: self
                .sources_per_context
                .min(Self::HARD_CAP.sources_per_context),
            first_level_entries: self
                .first_level_entries
                .min(Self::HARD_CAP.first_level_entries),
            bytes_per_source: self.bytes_per_source.min(Self::HARD_CAP.bytes_per_source),
            bytes_per_refresh: self.bytes_per_refresh.min(Self::HARD_CAP.bytes_per_refresh),
            refresh_budget_ms: self.refresh_budget_ms.min(Self::HARD_CAP.refresh_budget_ms),
            cli_output_bytes: self.cli_output_bytes.min(Self::HARD_CAP.cli_output_bytes),
            cli_concurrency: self.cli_concurrency.min(Self::HARD_CAP.cli_concurrency),
            watchers: self.watchers.min(Self::HARD_CAP.watchers),
            diagnostics: self.diagnostics.min(Self::HARD_CAP.diagnostics),
        }
    }
}

impl Default for AgentAssetLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetSource {
    pub id: String,
    pub context_id: String,
    pub label: String,
    pub scope: AgentAssetScope,
    pub origin: AgentAssetInstallationOrigin,
    pub environment_id: String,
    pub workspace_id: Option<String>,
    pub path: String,
    /// Adapter-owned access boundary used only by the Rust opaque-path
    /// resolver. It must never cross IPC or become frontend authority.
    #[serde(skip)]
    pub allowed_root: String,
    pub precedence: u32,
    pub writable: bool,
    pub sensitive: bool,
    pub source_kind: AgentAssetSourceKind,
    pub categories: Vec<AgentAssetCategory>,
    pub revision: AgentAssetRevision,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
    pub access: AgentAssetAccess,
    pub actions: Vec<AgentAssetAction>,
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
    pub executable_identity: Option<AgentExecutableIdentity>,
    pub executable_revision: Option<String>,
    pub installed_version: Option<String>,
    pub discovery_source: AgentDiscoverySource,
    pub distribution: AgentCliDistribution,
    pub channel: AgentInstallationChannel,
    pub installed_version_source: AgentVersionSource,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetOpenTarget {
    Asset,
    Reveal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetRecord {
    pub stable_id: String,
    pub agent_kind: AgentCliKind,
    pub category: AgentAssetCategory,
    pub native_id: String,
    pub label: String,
    pub source_ids: Vec<String>,
    pub inspection_source_id: String,
    pub scope: AgentAssetScope,
    pub provenance: Vec<AgentAssetProvenance>,
    pub environment_id: String,
    pub workspace_id: Option<String>,
    pub path: Option<String>,
    pub precedence: u32,
    pub writable: bool,
    pub declared_state: AgentAssetState,
    pub effective_state: AgentAssetState,
    pub trust_state: AgentTrustState,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
    pub revision: AgentAssetRevision,
    pub sensitive: bool,
    pub is_directory: bool,
    pub context_id: String,
    pub represented_declaration_ids: Vec<String>,
    pub resolution: AgentAssetResolution,
    pub relationships: AgentAssetRelationships,
    pub actions: Vec<AgentAssetAction>,
    pub access: AgentAssetAccess,
    pub compatible_installation_ids: Vec<String>,
    pub selected_action_installation_id: Option<String>,
    pub details: AgentAssetDetails,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHookRuleCount {
    pub agent_kind: AgentCliKind,
    pub rule_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEnvironmentInventory {
    pub environment: AgentEnvironmentDescriptor,
    pub installations: Vec<AgentInstallation>,
    pub sources: Vec<AgentAssetSource>,
    pub capabilities: Vec<AgentCapabilities>,
    pub assets: Vec<AgentAssetRecord>,
    pub hook_rule_counts: Vec<AgentHookRuleCount>,
    pub scanned_at: String,
    pub workspace: Option<String>,
    pub contexts: Vec<AgentConfigurationContext>,
    pub declarations: Vec<AgentAssetDeclaration>,
    pub limits: AgentAssetLimits,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
    pub mechanisms: Vec<AgentAssetMechanismRecord>,
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
    pub access_id: String,
    pub source_revision: AgentAssetRevision,
    pub path: String,
    pub content: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub truncated: bool,
    pub metadata_only: bool,
    pub diagnostics: Vec<AgentAssetReadDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssetReadDiagnostic {
    DirectoryMetadataOnly,
    SensitiveFileMetadataOnly,
    SensitiveValuesRedacted,
    UnsupportedSchemaMetadataOnly,
    InvalidDocumentMetadataOnly,
    ReadLimitMetadataOnly,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;

    fn assert_wire_round_trip<T>(value: T, expected: serde_json::Value)
    where
        T: DeserializeOwned + Serialize,
    {
        assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        let decoded = serde_json::from_value::<T>(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }

    #[test]
    fn asset_install_state_uses_explicit_wire_values() {
        assert_eq!(
            serde_json::to_value(AgentAssetInstallState::Installed).unwrap(),
            "installed"
        );
        assert_eq!(
            serde_json::to_value(AgentAssetInstallState::NotInstalled).unwrap(),
            "notInstalled"
        );
        assert_eq!(
            serde_json::to_value(AgentAssetInstallState::Unknown).unwrap(),
            "unknown"
        );

        let value = serde_json::to_value(AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::Unknown,
            enabled: AgentAssetDeclaredState::Unknown,
            trusted: AgentTrustState::Unknown,
        })
        .unwrap();
        assert_eq!(value["installState"], "unknown");
        assert!(value.get("install_state").is_none());
        assert!(value.get("installed").is_none());
    }

    #[test]
    fn asset_details_use_exact_camel_case_wire_fields_for_every_variant() {
        let cases = [
            (
                AgentAssetDetails::Skill {
                    enabled: AgentAssetDeclaredState::Enabled,
                    invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
                },
                serde_json::json!({
                    "kind": "skill",
                    "enabled": "enabled",
                    "invocationPolicy": "modelInvocable",
                }),
            ),
            (
                AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::WebSocket,
                    declared_state: AgentAssetDeclaredState::Pending,
                    approval_state: AgentMcpApprovalState::Approved,
                    effective_availability: AgentAssetEffectiveAvailability::Available,
                },
                serde_json::json!({
                    "kind": "mcp",
                    "transport": "webSocket",
                    "declaredState": "pending",
                    "approvalState": "approved",
                    "effectiveAvailability": "available",
                }),
            ),
            (
                AgentAssetDetails::Plugin {
                    install_state: AgentAssetInstallState::Installed,
                    enabled: AgentAssetDeclaredState::Disabled,
                    trusted: AgentTrustState::Required,
                },
                serde_json::json!({
                    "kind": "plugin",
                    "installState": "installed",
                    "enabled": "disabled",
                    "trusted": "required",
                }),
            ),
            (
                AgentAssetDetails::Extension {
                    install_state: AgentAssetInstallState::NotInstalled,
                    enabled: AgentAssetDeclaredState::Unknown,
                    trusted: AgentTrustState::Unknown,
                },
                serde_json::json!({
                    "kind": "extension",
                    "installState": "notInstalled",
                    "enabled": "unknown",
                    "trusted": "unknown",
                }),
            ),
            (
                AgentAssetDetails::Hook {
                    managed: true,
                    enabled: AgentAssetDeclaredState::Rejected,
                    rule_count: Some(3),
                },
                serde_json::json!({
                    "kind": "hook",
                    "managed": true,
                    "enabled": "rejected",
                    "ruleCount": 3,
                }),
            ),
            (
                AgentAssetDetails::StatusUi {
                    mode: AgentStatusUiMode::Command,
                    command_present: true,
                },
                serde_json::json!({
                    "kind": "statusUi",
                    "mode": "command",
                    "commandPresent": true,
                }),
            ),
        ];

        for (value, expected) in cases {
            assert_wire_round_trip(value, expected);
        }
    }

    #[test]
    fn hook_rule_counts_distinguish_zero_from_unknown_in_camel_case_ipc() {
        for rule_count in [Some(0), Some(5), None] {
            let value = serde_json::to_value(AgentHookRuleCount {
                agent_kind: AgentCliKind::Codex,
                rule_count,
            })
            .unwrap();
            assert_eq!(
                value,
                serde_json::json!({"agentKind":"codex", "ruleCount":rule_count})
            );
            let details = serde_json::to_value(AgentAssetDetails::Hook {
                managed: false,
                enabled: AgentAssetDeclaredState::Unknown,
                rule_count,
            })
            .unwrap();
            assert_eq!(details["ruleCount"], value["ruleCount"]);
            assert!(details.get("rule_count").is_none());
        }
    }

    #[test]
    fn asset_policy_references_use_exact_camel_case_wire_fields() {
        let cases = [
            (
                AgentAssetPolicyReference::Declaration {
                    declaration_id: "declaration:mcp:one".to_string(),
                },
                serde_json::json!({
                    "kind": "declaration",
                    "declarationId": "declaration:mcp:one",
                }),
            ),
            (
                AgentAssetPolicyReference::Source {
                    source_id: "source:managed:mcp".to_string(),
                },
                serde_json::json!({
                    "kind": "source",
                    "sourceId": "source:managed:mcp",
                }),
            ),
        ];

        for (value, expected) in cases {
            assert_wire_round_trip(value, expected);
        }
    }

    #[test]
    fn asset_resolution_uses_orthogonal_wire_fields() {
        let value = AgentAssetResolution {
            relation: AgentAssetResolutionRelation::ReplaceWinner,
            qualified_collision: true,
            terminal: Some(AgentAssetResolutionTerminal::PolicyBlocked),
            contributor_ids: vec!["declaration:one".to_owned()],
            winner_id: Some("asset:winner".to_owned()),
            control_source: Some(AgentAssetPolicyReference::Source {
                source_id: "source:policy".to_owned(),
            }),
            diagnostics: Vec::new(),
        };
        assert_wire_round_trip(
            value,
            serde_json::json!({
                "relation": "replaceWinner",
                "qualifiedCollision": true,
                "terminal": "policyBlocked",
                "contributorIds": ["declaration:one"],
                "winnerId": "asset:winner",
                "controlSource": {
                    "kind": "source",
                    "sourceId": "source:policy"
                },
                "diagnostics": []
            }),
        );
    }

    #[test]
    fn asset_resolution_participation_uses_exact_tagged_wire_values() {
        let cases = [
            (
                AgentAssetResolutionParticipation::Participates,
                serde_json::json!({ "kind": "participates" }),
            ),
            (
                AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace,
                },
                serde_json::json!({
                    "kind": "suppressed",
                    "reason": "untrustedWorkspace",
                }),
            ),
        ];

        for (value, expected) in cases {
            assert_wire_round_trip(value, expected);
        }
    }

    #[test]
    fn skill_frontmatter_diagnostics_use_yaml_and_exact_limit_wire_values() {
        assert_wire_round_trip(
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some("frontmatter.syntax".to_owned()),
            },
            serde_json::json!({
                "kind": "malformed",
                "format": "yaml",
                "location": "frontmatter.syntax",
            }),
        );
        for (limit, wire, accepted) in [
            (
                AgentAssetLimitKind::FrontmatterLines,
                "frontmatterLines",
                64,
            ),
            (
                AgentAssetLimitKind::FrontmatterBytes,
                "frontmatterBytes",
                16384,
            ),
        ] {
            assert_wire_round_trip(
                AgentAssetDiagnostic::Truncated {
                    limit,
                    accepted,
                    observed_at_least: accepted + 1,
                },
                serde_json::json!({
                    "kind": "truncated",
                    "limit": wire,
                    "accepted": accepted,
                    "observedAtLeast": accepted + 1,
                }),
            );
        }
    }

    #[test]
    fn asset_diagnostics_use_exact_camel_case_wire_fields_for_every_variant() {
        let cases = [
            (
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::Diagnostics,
                    accepted: 10,
                    observed_at_least: 11,
                },
                serde_json::json!({
                    "kind": "truncated",
                    "limit": "diagnostics",
                    "accepted": 10,
                    "observedAtLeast": 11,
                }),
            ),
            (
                AgentAssetDiagnostic::Malformed {
                    format: AgentAssetDocumentFormat::Json,
                    location: Some("settings.json".to_string()),
                },
                serde_json::json!({
                    "kind": "malformed",
                    "format": "json",
                    "location": "settings.json",
                }),
            ),
            (
                AgentAssetDiagnostic::DuplicateNativeId {
                    category: AgentAssetCategory::Mcp,
                    native_id: "server:one".to_string(),
                },
                serde_json::json!({
                    "kind": "duplicateNativeId",
                    "category": "mcp",
                    "nativeId": "server:one",
                }),
            ),
            (
                AgentAssetDiagnostic::UnknownField {
                    field_path: "mcp.servers.one.extra".to_string(),
                },
                serde_json::json!({
                    "kind": "unknownField",
                    "fieldPath": "mcp.servers.one.extra",
                }),
            ),
            (
                AgentAssetDiagnostic::SymlinkRejected {
                    source_id: "source:link".to_string(),
                },
                serde_json::json!({
                    "kind": "symlinkRejected",
                    "sourceId": "source:link",
                }),
            ),
            (
                AgentAssetDiagnostic::BudgetExceeded {
                    elapsed_ms: 6_000,
                    budget_ms: 5_000,
                },
                serde_json::json!({
                    "kind": "budgetExceeded",
                    "elapsedMs": 6000,
                    "budgetMs": 5000,
                }),
            ),
            (
                AgentAssetDiagnostic::ReadFailed {
                    source_id: "source:settings".to_string(),
                    error_kind: AgentAssetIoErrorKind::PermissionDenied,
                },
                serde_json::json!({
                    "kind": "readFailed",
                    "sourceId": "source:settings",
                    "errorKind": "permissionDenied",
                }),
            ),
            (
                AgentAssetDiagnostic::InvalidNativeId {
                    category: AgentAssetCategory::Skill,
                },
                serde_json::json!({
                    "kind": "invalidNativeId",
                    "category": "skill",
                }),
            ),
            (
                AgentAssetDiagnostic::UnresolvedRelationship {
                    relation: AgentAssetRelationKind::ProvidedBy,
                    native_id: "plugin:provider".to_string(),
                },
                serde_json::json!({
                    "kind": "unresolvedRelationship",
                    "relation": "providedBy",
                    "nativeId": "plugin:provider",
                }),
            ),
            (
                AgentAssetDiagnostic::InvalidProjection {
                    projection_key: "mcp:server:one".to_string(),
                },
                serde_json::json!({
                    "kind": "invalidProjection",
                    "projectionKey": "mcp:server:one",
                }),
            ),
            (
                AgentAssetDiagnostic::InvalidResolution {
                    projection_key: "mcp:server:one".to_string(),
                    resolution: AgentAssetResolutionRelation::Unknown,
                },
                serde_json::json!({
                    "kind": "invalidResolution",
                    "projectionKey": "mcp:server:one",
                    "resolution": "unknown",
                }),
            ),
            (
                AgentAssetDiagnostic::InstallationProbeFailed {
                    candidate_source: AgentDiscoverySource::Automatic,
                    error_kind: AgentExecutableProbeErrorKind::ChangedDuringProbe,
                },
                serde_json::json!({
                    "kind": "installationProbeFailed",
                    "candidateSource": "automatic",
                    "errorKind": "changedDuringProbe",
                }),
            ),
            (
                AgentAssetDiagnostic::SourceOutsideAllowedRoot {
                    source_id: "source:outside".to_string(),
                },
                serde_json::json!({
                    "kind": "sourceOutsideAllowedRoot",
                    "sourceId": "source:outside",
                }),
            ),
            (
                AgentAssetDiagnostic::SourceTypeMismatch {
                    source_id: "source:config".to_string(),
                    expected: AgentAssetSourceKind::File,
                    actual: AgentAssetSourceKind::Directory,
                },
                serde_json::json!({
                    "kind": "sourceTypeMismatch",
                    "sourceId": "source:config",
                    "expected": "file",
                    "actual": "directory",
                }),
            ),
            (
                AgentAssetDiagnostic::InvalidCompatibleInstallation {
                    installation_id: "installation:codex:one".to_string(),
                },
                serde_json::json!({
                    "kind": "invalidCompatibleInstallation",
                    "installationId": "installation:codex:one",
                }),
            ),
            (
                AgentAssetDiagnostic::DeclarationSuppressed {
                    reason: AgentAssetSuppressionReason::CompatibilitySourceDisabled,
                },
                serde_json::json!({
                    "kind": "declarationSuppressed",
                    "reason": "compatibilitySourceDisabled",
                }),
            ),
            (
                AgentAssetDiagnostic::PolicyBlocked,
                serde_json::json!({ "kind": "policyBlocked" }),
            ),
        ];

        assert_eq!(cases.len(), 17);
        for (value, expected) in cases {
            assert_wire_round_trip(value, expected);
        }
    }

    fn assert_legacy_fields_are_rejected<T>(cases: impl IntoIterator<Item = serde_json::Value>)
    where
        T: DeserializeOwned,
    {
        for value in cases {
            assert!(
                serde_json::from_value::<T>(value.clone()).is_err(),
                "legacy or snake_case field was accepted: {value}"
            );
        }
    }

    #[test]
    fn asset_details_reject_legacy_and_snake_case_fields() {
        assert_legacy_fields_are_rejected::<AgentAssetDetails>([
            serde_json::json!({
                "kind": "skill",
                "enabled": "enabled",
                "invocation_policy": "modelInvocable",
            }),
            serde_json::json!({
                "kind": "mcp",
                "transport": "stdio",
                "declared_state": "enabled",
                "approvalState": "approved",
                "effectiveAvailability": "available",
            }),
            serde_json::json!({
                "kind": "mcp",
                "transport": "stdio",
                "declaredState": "enabled",
                "approval_state": "approved",
                "effectiveAvailability": "available",
            }),
            serde_json::json!({
                "kind": "mcp",
                "transport": "stdio",
                "declaredState": "enabled",
                "approvalState": "approved",
                "effective_availability": "available",
            }),
            serde_json::json!({
                "kind": "plugin",
                "install_state": "installed",
                "enabled": "enabled",
                "trusted": "trusted",
            }),
            serde_json::json!({
                "kind": "plugin",
                "installed": true,
                "enabled": "enabled",
                "trusted": "trusted",
            }),
            serde_json::json!({
                "kind": "extension",
                "install_state": "installed",
                "enabled": "enabled",
                "trusted": "trusted",
            }),
            serde_json::json!({
                "kind": "extension",
                "installed": true,
                "enabled": "enabled",
                "trusted": "trusted",
            }),
            serde_json::json!({
                "kind": "statusUi",
                "mode": "command",
                "command_present": true,
            }),
        ]);
    }

    #[test]
    fn asset_policy_references_reject_snake_case_fields() {
        assert_legacy_fields_are_rejected::<AgentAssetPolicyReference>([
            serde_json::json!({
                "kind": "declaration",
                "declaration_id": "declaration:mcp:one",
            }),
            serde_json::json!({
                "kind": "source",
                "source_id": "source:managed:mcp",
            }),
        ]);
    }

    #[test]
    fn asset_diagnostics_reject_snake_case_fields_for_each_affected_variant() {
        assert_legacy_fields_are_rejected::<AgentAssetDiagnostic>([
            serde_json::json!({
                "kind": "truncated",
                "limit": "diagnostics",
                "accepted": 10,
                "observed_at_least": 11,
            }),
            serde_json::json!({
                "kind": "duplicateNativeId",
                "category": "mcp",
                "native_id": "server:one",
            }),
            serde_json::json!({
                "kind": "unknownField",
                "field_path": "mcp.servers.one.extra",
            }),
            serde_json::json!({
                "kind": "symlinkRejected",
                "source_id": "source:link",
            }),
            serde_json::json!({
                "kind": "budgetExceeded",
                "elapsed_ms": 6000,
                "budgetMs": 5000,
            }),
            serde_json::json!({
                "kind": "budgetExceeded",
                "elapsedMs": 6000,
                "budget_ms": 5000,
            }),
            serde_json::json!({
                "kind": "readFailed",
                "source_id": "source:settings",
                "errorKind": "permissionDenied",
            }),
            serde_json::json!({
                "kind": "readFailed",
                "sourceId": "source:settings",
                "error_kind": "permissionDenied",
            }),
            serde_json::json!({
                "kind": "unresolvedRelationship",
                "relation": "providedBy",
                "native_id": "plugin:provider",
            }),
            serde_json::json!({
                "kind": "invalidProjection",
                "projection_key": "mcp:server:one",
            }),
            serde_json::json!({
                "kind": "invalidResolution",
                "projection_key": "mcp:server:one",
                "resolution": "qualifiedCollision",
            }),
            serde_json::json!({
                "kind": "installationProbeFailed",
                "candidate_source": "automatic",
                "errorKind": "changedDuringProbe",
            }),
            serde_json::json!({
                "kind": "installationProbeFailed",
                "candidateSource": "automatic",
                "error_kind": "changedDuringProbe",
            }),
            serde_json::json!({
                "kind": "sourceOutsideAllowedRoot",
                "source_id": "source:outside",
            }),
            serde_json::json!({
                "kind": "sourceTypeMismatch",
                "source_id": "source:config",
                "expected": "file",
                "actual": "directory",
            }),
            serde_json::json!({
                "kind": "invalidCompatibleInstallation",
                "installation_id": "installation:codex:one",
            }),
        ]);
    }
}
