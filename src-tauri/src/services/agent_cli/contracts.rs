mod codex_asset;
pub(crate) use codex_asset::{CodexAssetPayload, CodexSkillRule, CodexSkillSelector};

use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDetails, AgentAssetDiagnostic,
    AgentAssetInstallationOrigin, AgentAssetNativeRef, AgentAssetPresence,
    AgentAssetResolutionParticipation, AgentAssetResolutionRelation, AgentAssetRevision,
    AgentAssetScope, AgentAssetSourceKind, AgentAssetState, AgentAssetSuppressionReason,
    AgentCliKind, AgentConfigurationContext, AgentInstallation, AgentTrustState, CliConfigSnapshot,
    CliSessionDetail, CliSessionMessageRole, CliSessionSummary, Provider, TemporaryCliSessionMode,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    ops::ControlFlow,
    path::{Path, PathBuf},
};

/// Adapter-owned native data that is needed to resolve an asset without
/// exposing the original configuration values through the public model.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AgentMcpMatcherIdentity {
    Stdio { argv: Vec<String> },
    Remote { url: String },
}

impl fmt::Debug for AgentMcpMatcherIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (variant, argument_count) = match self {
            Self::Stdio { argv } => ("Stdio", argv.len()),
            Self::Remote { .. } => ("Remote", 0),
        };
        formatter
            .debug_struct("AgentMcpMatcherIdentity")
            .field("variant", &variant)
            .field("argument_count", &argument_count)
            .finish()
    }
}

/// Adapter-owned MCP policy metadata. Members are normalized by the owning
/// adapter before entering the common contract; raw policy objects remain in
/// the bounded source snapshot and are never copied into this payload.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AgentMcpPolicyPayload {
    Allowed(BTreeSet<String>),
    Excluded(BTreeSet<String>),
    InvalidAllowed,
    InvalidExcluded,
}

impl fmt::Debug for AgentMcpPolicyPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("AgentMcpPolicyPayload");
        match self {
            Self::Allowed(members) => debug
                .field("policy_kind", &"Allowed")
                .field("member_count", &members.len()),
            Self::Excluded(members) => debug
                .field("policy_kind", &"Excluded")
                .field("member_count", &members.len()),
            Self::InvalidAllowed => debug.field("policy_kind", &"InvalidAllowed"),
            Self::InvalidExcluded => debug.field("policy_kind", &"InvalidExcluded"),
        }
        .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeminiExtensionEnablementUnknownCause {
    InvalidEntry,
    WorkspaceUnavailable,
}

/// Codex requirements retain their native set lifecycle and matcher identity.
/// The root is emitted only after the whole allowlist was decoded and offered
/// to the bounded parser output. An absent root is not an empty allowlist.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum CodexRequirementsPayload {
    MissingFile,
    NoPolicy,
    AllowlistRoot { entry_count: usize },
    InvalidRequirements,
    Entry(CodexMcpRequirement),
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct CodexMcpRequirement {
    pub(crate) identity: CodexRawMcpServerIdentity,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexMcpServerCommandRequirement {
    pub(crate) command: CodexMcpServerCommandMatcher,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexMcpServerUrlRequirement {
    pub(crate) url: CodexMcpServerValueMatcher,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum CodexRawMcpServerIdentity {
    LegacyCommand { command: String },
    LegacyUrl { url: String },
    CommandMatcher(CodexMcpServerCommandRequirement),
    UrlMatcher(CodexMcpServerUrlRequirement),
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexMcpServerCommandMatcher {
    pub(crate) executable: String,
    pub(crate) args: Vec<CodexMcpServerValueMatcher>,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "match", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CodexMcpServerValueMatcher {
    Exact { value: String },
    Prefix { value: String },
    Regex { expression: String },
}

impl fmt::Debug for CodexMcpRequirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, argument_count) = match &self.identity {
            CodexRawMcpServerIdentity::LegacyCommand { .. } => ("LegacyCommand", 0),
            CodexRawMcpServerIdentity::LegacyUrl { .. } => ("LegacyUrl", 0),
            CodexRawMcpServerIdentity::CommandMatcher(command) => {
                ("CommandMatcher", command.command.args.len())
            }
            CodexRawMcpServerIdentity::UrlMatcher(_) => ("UrlMatcher", 0),
        };
        formatter
            .debug_struct("CodexMcpRequirement")
            .field("identity_kind", &kind)
            .field("argument_count", &argument_count)
            .finish()
    }
}

impl fmt::Debug for CodexMcpServerValueMatcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Exact { .. } => "Exact(<redacted>)",
            Self::Prefix { .. } => "Prefix(<redacted>)",
            Self::Regex { .. } => "Regex(<redacted>)",
        })
    }
}

impl fmt::Debug for CodexRequirementsPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFile => formatter.write_str("MissingFile"),
            Self::NoPolicy => formatter.write_str("NoPolicy"),
            Self::AllowlistRoot { entry_count } => formatter
                .debug_struct("AllowlistRoot")
                .field("entry_count", entry_count)
                .finish(),
            Self::InvalidRequirements => formatter.write_str("InvalidRequirements"),
            Self::Entry(requirement) => formatter.debug_tuple("Entry").field(requirement).finish(),
        }
    }
}

/// Exact native matcher used by Gemini Hook definitions and disabled policy.
/// The value is intentionally private so display/IPC code cannot accidentally
/// turn a command, name, or native sentinel into public inventory evidence.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AgentHookDisableMatcherIdentity {
    value: String,
}

impl AgentHookDisableMatcherIdentity {
    pub(crate) fn new(value: String) -> Self {
        Self { value }
    }
}

impl fmt::Debug for AgentHookDisableMatcherIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentHookDisableMatcherIdentity")
            .field("redacted", &true)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AgentHookPolicyPayload {
    DisabledSet(BTreeSet<AgentHookDisableMatcherIdentity>),
    InvalidDisabledSet,
    /// Gemini's whole-system switch; absence emits no declaration.
    GlobalEnabled(Option<bool>),
}

/// The native adapter decides scope, union/precedence and exact-name matching.
/// A disabled set is a complete source witness, not a synthetic asset row.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AgentSkillPolicyPayload {
    DisabledSet(BTreeSet<String>),
    InvalidDisabledSet,
}

impl fmt::Debug for AgentSkillPolicyPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DisabledSet(names) => formatter
                .debug_struct("SkillDisabledSet")
                .field("member_count", &names.len())
                .finish(),
            Self::InvalidDisabledSet => formatter.write_str("InvalidSkillDisabledSet"),
        }
    }
}

/// Typed provenance for an MCP definition.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentMcpDefinitionOrigin {
    Declared,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AgentMcpDefinitionPayload {
    pub(crate) identity: AgentMcpMatcherIdentity,
    pub(crate) origin: AgentMcpDefinitionOrigin,
}

impl fmt::Debug for AgentMcpDefinitionOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Declared => "Declared",
        };
        formatter.write_str(value)
    }
}

impl fmt::Debug for AgentMcpDefinitionPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentMcpDefinitionPayload")
            .field("origin", &self.origin)
            .field("identity", &self.identity)
            .finish()
    }
}

/// Source-level invalid control evidence. This is intentionally private: it
/// lets resolution distinguish an unreadable control file from an absent one
/// without exposing raw snapshots or inventing public inventory fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAssetInvalidControl {
    McpEnablement,
    ExtensionEnablement,
}

impl fmt::Debug for AgentAssetInvalidControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::McpEnablement => "McpEnablement",
            Self::ExtensionEnablement => "ExtensionEnablement",
        };
        formatter
            .debug_struct("AgentAssetInvalidControl")
            .field("control_kind", &kind)
            .finish()
    }
}

impl fmt::Debug for AgentHookPolicyPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("AgentHookPolicyPayload");
        match self {
            Self::DisabledSet(members) => debug
                .field("policy_kind", &"DisabledSet")
                .field("member_count", &members.len()),
            Self::InvalidDisabledSet => debug.field("policy_kind", &"InvalidDisabledSet"),
            Self::GlobalEnabled(value) => debug
                .field("policy_kind", &"GlobalEnabled")
                .field("valid", &value.is_some()),
        }
        .finish()
    }
}

#[derive(Clone, Default)]
pub(crate) enum AgentAssetNativePayload {
    #[default]
    None,
    TomlTable(toml::map::Map<String, toml::Value>),
    McpDefinition(AgentMcpDefinitionPayload),
    McpPolicy(AgentMcpPolicyPayload),
    CodexRequirements(CodexRequirementsPayload),
    CodexAsset(CodexAssetPayload),
    CodexHook(crate::services::agent_cli::codex::CodexHookPayload),
    ClaudeControl(crate::services::agent_cli::claude::ClaudeControlPayload),
    ClaudePlugin(crate::services::agent_cli::claude::ClaudePluginPayload),
    GrokPlugin(crate::services::agent_cli::grok::GrokPluginPayload),
    GrokHook(crate::services::agent_cli::grok::GrokHookPayload),
    GrokMcpDefinition {
        enabled: Option<bool>,
        valid: bool,
    },
    GrokSkillDefinition,
    GrokStateControl(GrokStateControl),
    GeminiMcpEnablementInvalid,
    GeminiExtensionEnablementUnknown(GeminiExtensionEnablementUnknownCause),
    HookDefinition(AgentHookDisableMatcherIdentity),
    HookPolicy(AgentHookPolicyPayload),
    SkillPolicy(AgentSkillPolicyPayload),
    InvalidControl(AgentAssetInvalidControl),
}

impl fmt::Debug for AgentAssetNativePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"None")
                .finish(),
            Self::TomlTable(table) => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"TomlTable")
                .field("field_count", &table.len())
                .finish(),
            Self::McpDefinition(payload) => {
                let (variant, count) = match &payload.identity {
                    AgentMcpMatcherIdentity::Stdio { argv } => ("Stdio", argv.len()),
                    AgentMcpMatcherIdentity::Remote { .. } => ("Remote", 0),
                };
                formatter
                    .debug_struct("AgentAssetNativePayload")
                    .field("variant", &variant)
                    .field("argument_count", &count)
                    .field("origin", &payload.origin)
                    .finish()
            }
            Self::McpPolicy(policy) => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"McpPolicy")
                .field("policy", policy)
                .finish(),
            Self::CodexRequirements(requirements) => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"CodexRequirements")
                .field("requirements", requirements)
                .finish(),
            Self::CodexAsset(asset) => formatter.debug_tuple("CodexAsset").field(asset).finish(),
            Self::CodexHook(_) => formatter.write_str("CodexHook"),
            Self::ClaudeControl(control) => formatter
                .debug_tuple("ClaudeControl")
                .field(control)
                .finish(),
            Self::ClaudePlugin(_) => formatter.write_str("ClaudePlugin"),
            Self::GrokPlugin(_) => formatter.write_str("GrokPlugin"),
            Self::GrokHook(_) => formatter.write_str("GrokHook"),
            Self::GrokMcpDefinition { enabled, valid } => formatter
                .debug_struct("GrokMcpDefinition")
                .field("enabled", enabled)
                .field("valid", valid)
                .finish(),
            Self::GrokSkillDefinition => formatter.write_str("GrokSkillDefinition"),
            Self::GrokStateControl(control) => formatter
                .debug_tuple("GrokStateControl")
                .field(control)
                .finish(),
            Self::GeminiMcpEnablementInvalid => formatter.write_str("GeminiMcpEnablementInvalid"),
            Self::GeminiExtensionEnablementUnknown(cause) => formatter
                .debug_tuple("GeminiExtensionEnablementUnknown")
                .field(cause)
                .finish(),
            Self::HookDefinition(matcher) => {
                let _ = matcher;
                formatter
                    .debug_struct("AgentAssetNativePayload")
                    .field("variant", &"HookDefinition")
                    .field("matcher_present", &true)
                    .finish()
            }
            Self::HookPolicy(policy) => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"HookPolicy")
                .field("policy", policy)
                .finish(),
            Self::SkillPolicy(policy) => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"SkillPolicy")
                .field("policy", policy)
                .finish(),
            Self::InvalidControl(control) => formatter
                .debug_struct("AgentAssetNativePayload")
                .field("variant", &"InvalidControl")
                .field("control", control)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrokStateControl {
    McpDisabled,
    PluginEnabled,
    PluginDisabled,
    SkillDisabled,
    InvalidMcp,
    InvalidPlugin,
    InvalidSkill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AgentAssetLogicalOrigin {
    pub scope: AgentAssetScope,
    pub precedence: u32,
}

impl AgentAssetSourceSpec {
    /// Snapshot-owned physical identity. Never resolve a pathname again while
    /// combining it with bytes from an earlier snapshot.
    pub(crate) fn definition_identity_path(&self) -> Option<&Path> {
        self.verified_physical_path.as_deref().or_else(|| {
            (!matches!(
                self.path_policy,
                AgentAssetSourcePathPolicy::ReadonlySkillLink { .. }
            ))
            .then_some(self.path.as_path())
        })
    }

    pub(crate) fn allows(&self, origin: AgentAssetLogicalOrigin) -> bool {
        self.allowed_logical_origins.contains(&origin)
    }

    pub(crate) fn logical_origins_valid(&self) -> bool {
        let origins = &self.allowed_logical_origins;
        !origins.is_empty() && origins.len() <= 8 && {
            let mut unique = origins.clone();
            unique.sort();
            unique.dedup();
            unique.len() == origins.len()
        }
    }

    /// Normalize the origin allow-list at the source admission boundary.
    /// Ordering is an implementation detail, while duplicates are ambiguous
    /// input and must be rejected rather than silently collapsed.
    pub(crate) fn normalize_logical_origins(&mut self) -> bool {
        if self.allowed_logical_origins.is_empty() || self.allowed_logical_origins.len() > 8 {
            return false;
        }
        self.allowed_logical_origins.sort();
        self.logical_origins_valid()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentAssetSourceSpec {
    pub native_source_key: String,
    pub label: String,
    pub scope: AgentAssetScope,
    pub origin: AgentAssetInstallationOrigin,
    /// Explicit native package provenance; installation location alone is not authorship.
    pub provider: crate::models::AgentAssetProviderOrigin,
    pub path: PathBuf,
    pub allowed_root: PathBuf,
    pub path_policy: AgentAssetSourcePathPolicy,
    pub verified_physical_path: Option<PathBuf>,
    pub precedence: u32,
    pub writable: bool,
    pub sensitive: bool,
    pub source_kind: AgentAssetSourceKind,
    pub categories: Vec<AgentAssetCategory>,
    /// A dedicated Hook policy source cannot add configured handlers. Its read
    /// failure affects enablement proof, not the independent configured count.
    pub hook_definition_source: bool,
    pub allowed_logical_origins: Vec<AgentAssetLogicalOrigin>,
}

/// Native opt-in for observed Skill directory references. Ordinary sources and
/// follow-ups retain the same no-follow boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum AgentAssetSourcePathPolicy {
    #[default]
    NoFollow,
    ReadonlySkillLinkRoot {
        shared_root: PathBuf,
    },
    ReadonlySkillLink {
        shared_root: PathBuf,
        manifest_path: PathBuf,
        manifest_revision: AgentAssetRevision,
        entry_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetDirectoryEntry {
    pub name: String,
    pub source_kind: AgentAssetSourceKind,
    pub is_symlink: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum AgentAssetSnapshot {
    Missing {
        revision: AgentAssetRevision,
    },
    File {
        bytes: Vec<u8>,
        revision: AgentAssetRevision,
    },
    DirectoryManifest {
        entries: Vec<AgentAssetDirectoryEntry>,
        revision: AgentAssetRevision,
        /// A follow-up source may only be requested from a complete manifest.
        /// Bounded manifests still expose their deterministic prefix for
        /// inventory, but are not authoritative for child discovery.
        complete: bool,
    },
    Blocked {
        revision: AgentAssetRevision,
        diagnostic: AgentAssetDiagnostic,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentContextDiscoveryRequest<'a> {
    pub environment_id: &'a str,
    pub agent_kind: AgentCliKind,
    pub home: &'a Path,
    pub workspace: Option<&'a Path>,
    pub installations: &'a [AgentInstallation],
    pub workspace_trust: AgentTrustState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentOutputStop {
    Deadline,
    SourceLimit,
    EntryLimit,
    InvalidOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentDiagnosticEmission {
    Accepted,
    Duplicate,
    Saturated,
}

pub(crate) trait AgentDiagnosticOutput {
    fn has_regular_capacity(&self) -> bool;
    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission;
}

pub(crate) trait InitialSourceOutput: AgentDiagnosticOutput {
    fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop>;

    /// Admit a native seed and read it through the shared bounded snapshot port.
    /// The same snapshot is retained for parsing; absence of a reader grants no
    /// authority to perform an independent filesystem read. A stopped source
    /// output remains distinct from an admitted seed without a reader.
    fn snapshot_initial(
        &mut self,
        source: AgentAssetSourceSpec,
    ) -> ControlFlow<AgentOutputStop, Option<AgentAssetSnapshot>> {
        self.emit_initial(source)?;
        ControlFlow::Continue(None)
    }
}

pub(crate) trait FollowUpSourceOutput: AgentDiagnosticOutput {
    fn emit_follow_up(&mut self, source: AgentFollowUpSourceSpec) -> ControlFlow<AgentOutputStop>;
}

pub(crate) trait AgentParseOutput: AgentDiagnosticOutput {
    fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop>;
}

pub(crate) trait AgentResolveOutput: AgentDiagnosticOutput {
    fn emit_draft(&mut self, value: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop>;
}

pub(crate) type AgentContextDiscovery = for<'a> fn(
    AgentContextDiscoveryRequest<'a>,
    &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentWorkspaceTrustSourceRequest<'a> {
    pub home: &'a Path,
    pub workspace: &'a Path,
    pub config_root: &'a Path,
}

#[derive(Clone, Copy)]
pub(crate) struct AgentWorkspaceTrustResolveRequest<'a> {
    pub workspace: &'a Path,
    /// The caller's lexical spelling is retained alongside the canonical
    /// workspace path. Adapters may use both native lookup forms without
    /// exposing either as a public model field.
    pub workspace_lexical: Option<&'a Path>,
    pub sources: &'a [AgentAssetResolveSource<'a>],
}

pub(crate) type AgentWorkspaceTrustSourceDiscovery =
    for<'a> fn(AgentWorkspaceTrustSourceRequest<'a>, &mut dyn InitialSourceOutput);
pub(crate) type AgentWorkspaceTrustResolver = for<'a> fn(
    AgentWorkspaceTrustResolveRequest<'a>,
    &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentSourceDiscoveryRequest<'a> {
    pub context: &'a AgentConfigurationContext,
    pub home: &'a Path,
    pub workspace: Option<&'a Path>,
    pub installations: &'a [AgentInstallation],
}

pub(crate) type AgentSourceDiscovery =
    for<'a> fn(AgentSourceDiscoveryRequest<'a>, &mut dyn InitialSourceOutput);

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentDefinitionSelectionRequest<'a> {
    pub context: &'a AgentConfigurationContext,
    pub sources: &'a [AgentAssetSourceSpec],
    pub declarations: &'a [ParsedAgentAsset],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentDefinitionSuppression {
    pub declaration_id: String,
    pub reason: AgentAssetSuppressionReason,
}

/// Native selection retains every observed declaration. Only definitions
/// independently proved redundant or owned by an unselected parent cease
/// participating in resolution; they remain in the complete public evidence.
pub(crate) type AgentDefinitionSelector =
    for<'a> fn(AgentDefinitionSelectionRequest<'a>) -> Vec<AgentDefinitionSuppression>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentFollowUpSourceTarget {
    ManifestFile {
        entry_name: String,
    },
    Descendant {
        directory_entry_name: String,
        relative_path: PathBuf,
    },
    ReadonlySkillDirectoryLink {
        directory_entry_name: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct AgentFollowUpSourceSpec {
    pub parent_source_key: String,
    pub target: AgentFollowUpSourceTarget,
    pub native_source_key: String,
    pub label: String,
    pub scope: AgentAssetScope,
    pub precedence: u32,
    pub sensitive: bool,
    pub source_kind: AgentAssetSourceKind,
    pub categories: Vec<AgentAssetCategory>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentFollowUpSourceDiscoveryRequest<'a> {
    pub parent: &'a AgentAssetSourceSpec,
    pub manifest: &'a [AgentAssetDirectoryEntry],
}

pub(crate) type AgentFollowUpSourceDiscovery =
    for<'a> fn(AgentFollowUpSourceDiscoveryRequest<'a>, &mut dyn FollowUpSourceOutput);

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentAssetParseRequest<'a> {
    pub context: &'a AgentConfigurationContext,
    pub source: &'a AgentAssetSourceSpec,
    pub snapshot: &'a AgentAssetSnapshot,
    /// Native user context, independent of a configurable Agent config root.
    /// Production inventory always supplies it; isolated decoders may omit it.
    pub native_home: Option<&'a Path>,
    pub workspace_canonical: Option<&'a Path>,
    pub workspace_lexical: Option<&'a Path>,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedAgentAsset {
    pub declaration_id: String,
    /// Adapter-owned semantic group used to prove that resolution covers every
    /// declaration. The common projector never derives this from a native ID.
    pub resolution_group_key: String,
    pub source_key: String,
    pub native_id: String,
    pub declaration_key: String,
    pub label: String,
    pub category: AgentAssetCategory,
    pub logical_origin: AgentAssetLogicalOrigin,
    pub presence: AgentAssetPresence,
    pub declared_state: crate::models::AgentAssetDeclaredState,
    pub trust_state: crate::models::AgentTrustState,
    pub role: AgentAssetDeclarationRole,
    pub participation: AgentAssetResolutionParticipation,
    pub provided_by: Option<AgentAssetNativeRef>,
    pub action_owner: Option<AgentAssetNativeRef>,
    pub explicitly_affected: Vec<AgentAssetNativeRef>,
    pub details: AgentAssetDetails,
    pub facts: BTreeMap<String, String>,
    pub native_payload: AgentAssetNativePayload,
}

pub(crate) type AgentAssetParser =
    for<'a> fn(AgentAssetParseRequest<'a>, &mut dyn AgentParseOutput);

#[derive(Clone, Copy)]
pub(crate) struct AgentAssetResolveRequest<'a> {
    pub context: &'a AgentConfigurationContext,
    pub declarations: &'a [ParsedAgentAsset],
    pub sources: &'a [AgentAssetResolveSource<'a>],
}

#[derive(Clone, Copy)]
pub(crate) struct AgentAssetResolveSource<'a> {
    pub spec: &'a AgentAssetSourceSpec,
    pub snapshot: &'a AgentAssetSnapshot,
}

/// A native assessment receives only independently admitted identities and the
/// complete parser/source indexes. Drafts and their claimed proof never cross
/// this boundary.
pub(crate) struct AgentAssetAssessmentRequest<'a> {
    pub(crate) context: &'a AgentConfigurationContext,
    pub(crate) targets: &'a [AgentAssetAssessmentTarget],
    pub(crate) declarations: &'a [ParsedAgentAsset],
    pub(crate) sources: &'a [AgentAssetSourceSpec],
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AgentAssetAssessmentTarget {
    pub(crate) category: AgentAssetCategory,
    pub(crate) resolution_group_key: String,
    pub(crate) exact_native_id: String,
    pub(crate) subject: AgentAssetAssessmentSubject,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentAssetAssessmentSubject {
    Bucket,
    Definition(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAssetAssessmentFailure {
    MissingTarget,
    DuplicateTarget,
    IncompleteInput,
    InvalidNativeInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentAssetAssessmentResult {
    Assessed {
        state: AgentAssetNativeAssessment,
        projection: Box<AgentAssetProjectionAssessment>,
    },
    Unsupported(AgentAssetAssessmentFailure),
}

/// Completed native selection, independent of the resolver's candidate draft.
/// Keeping this at the decision exit avoids inventing a structural selection
/// while an adapter is still evaluating declared state or control evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetProjectionAssessment {
    pub(crate) anchor_declaration_id: String,
    pub(crate) relation: AgentAssetResolutionRelation,
    pub(crate) effective: AgentAssetEffectiveStateProofDraft,
    pub(crate) relationships: AgentAssetResolvedRelationships,
}

/// Duplicate results are sticky failures, including a third insertion. There
/// is intentionally no mutable-map or overwriting entry API.
#[derive(Debug, Default)]
pub(crate) struct AgentAssetAssessmentIndex {
    values: BTreeMap<AgentAssetAssessmentTarget, AgentAssetAssessmentResult>,
}

impl AgentAssetAssessmentIndex {
    pub(crate) fn insert(
        &mut self,
        target: AgentAssetAssessmentTarget,
        result: AgentAssetAssessmentResult,
    ) {
        use std::collections::btree_map::Entry;
        match self.values.entry(target) {
            Entry::Vacant(entry) => {
                entry.insert(result);
            }
            Entry::Occupied(mut entry) => {
                entry.insert(AgentAssetAssessmentResult::Unsupported(
                    AgentAssetAssessmentFailure::DuplicateTarget,
                ));
            }
        }
    }

    pub(crate) fn get(
        &self,
        target: &AgentAssetAssessmentTarget,
    ) -> Option<&AgentAssetAssessmentResult> {
        self.values.get(target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetNativeAssessment {
    pub(crate) declared_state: crate::models::AgentAssetDeclaredState,
    pub(crate) declared: AgentAssetDeclaredStateProofDraft,
    pub(crate) declared_members: Vec<AgentAssetEvidenceMember>,
    pub(crate) intrinsic: AgentAssetIntrinsicBasis,
    pub(crate) control: Option<AgentAssetControlAssessment>,
}

/// Native details after declared-state, approval and trust selection, before
/// parent gates, terminal control or shadowing. MCP availability is intrinsic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetIntrinsicBasis {
    pub(crate) details: crate::models::AgentAssetDetails,
    pub(crate) trust_state: crate::models::AgentTrustState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetEvidenceMember {
    pub(crate) declaration_id: String,
    pub(crate) expected_role: AgentAssetDeclarationRole,
    pub(crate) kind: AgentAssetEvidenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAssetEvidenceKind {
    Definition,
    Overlay {
        scope: AgentAssetStateOverlayScopeDraft,
    },
    InvalidStateControl {
        scope: AgentAssetStateOverlayScopeDraft,
    },
    Policy,
    /// Invalid effective control can be a state or policy declaration. Its
    /// target applicability is native-assessed, including global controls.
    InvalidControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetControlAssessment {
    pub(crate) cause: AgentAssetTerminalCauseDraft,
    pub(crate) members: Vec<AgentAssetEvidenceMember>,
    pub(crate) authorities: BTreeSet<AgentAssetControlAuthority>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentAssetControlAuthority {
    Declaration(String),
    SourceAggregate(String),
}

pub(crate) type AgentAssetStateAssessor =
    for<'a> fn(AgentAssetAssessmentRequest<'a>) -> AgentAssetAssessmentIndex;

#[derive(Debug, Clone)]
pub(crate) struct AgentAssetProjectedDraft {
    pub projection_key: String,
    /// Must match the adapter-owned group on every contributor.
    pub resolution_group_key: String,
    pub native_kind: AgentAssetCategory,
    pub native_id: String,
    pub label: String,
    pub declared_state: crate::models::AgentAssetDeclaredState,
    pub effective_state: AgentAssetState,
    pub trust_state: crate::models::AgentTrustState,
    pub inspection_source_id: String,
    pub represented_declaration_ids: Vec<String>,
    pub contributor_ids: Vec<String>,
    pub resolution: AgentAssetResolutionDraft,
    pub details: AgentAssetDetails,
    pub provided_by: Option<AgentAssetNativeRef>,
    pub action_owner: Option<AgentAssetNativeRef>,
    pub explicitly_affected: Vec<AgentAssetNativeRef>,
    /// Closed private proof of both state derivation axes.
    pub(crate) state_proof: AgentAssetStateProofDraft,
}

/// Fully resolved input for the common draft finalizer.  Adapters provide all
/// state-coupled values together; the finalizer only normalizes identity lists
/// and performs cheap local shape checks.
pub(crate) struct AgentAssetResolvedDraftInput<'a> {
    pub(crate) anchor: &'a ParsedAgentAsset,
    pub(crate) projection_key: String,
    pub(crate) represented: &'a [&'a ParsedAgentAsset],
    pub(crate) contributors: &'a [&'a ParsedAgentAsset],
    pub(crate) declared_state: crate::models::AgentAssetDeclaredState,
    pub(crate) effective_state: AgentAssetState,
    pub(crate) trust_state: crate::models::AgentTrustState,
    pub(crate) resolution: AgentAssetResolutionDraft,
    pub(crate) details: AgentAssetDetails,
    pub(crate) relationships: AgentAssetResolvedRelationships,
    pub(crate) state_proof: AgentAssetStateProofDraft,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentAssetResolvedRelationships {
    pub(crate) provided_by: Option<AgentAssetNativeRef>,
    pub(crate) action_owner: Option<AgentAssetNativeRef>,
    pub(crate) explicitly_affected: Vec<AgentAssetNativeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentAssetStateProofDraft {
    pub(crate) declared: AgentAssetDeclaredStateProofDraft,
    pub(crate) effective: AgentAssetEffectiveStateProofDraft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentAssetDeclaredStateProofDraft {
    Definition {
        declaration_ids: Vec<String>,
        selected_id: String,
    },
    NativeDefault {
        definition_id: String,
        outcome: crate::models::AgentAssetDeclaredState,
    },
    Overlay {
        declaration_ids: Vec<String>,
        scope: AgentAssetStateOverlayScopeDraft,
        outcome: crate::models::AgentAssetDeclaredState,
    },
    Policy {
        declaration_ids: Vec<String>,
        outcome: crate::models::AgentAssetDeclaredState,
    },
    Unknown {
        evidence: Vec<AgentAssetStateEvidenceRefDraft>,
        cause: AgentAssetDeclaredUnknownCauseDraft,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAssetStateOverlayScopeDraft {
    ExactNativeId,
    ResolutionGroup,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentAssetStateEvidenceRefDraft {
    Declaration { declaration_id: String },
    Source { source_key: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAssetDeclaredUnknownCauseDraft {
    StructuralConflict,
    OverlayConflict,
    InvalidTypedControl,
    InvalidNativeMerge,
    IntrinsicUnknown,
    ContextUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentAssetTerminalCauseDraft {
    TypedPolicy,
    InvalidControl,
    DeclaredUnknown,
    StructuralUnknown,
    ParentUnknown,
    TrustSuppressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentAssetEffectiveStateProofDraft {
    Intrinsic,
    ParentGate {
        parent: AgentAssetNativeRef,
        input: Box<Self>,
    },
    Terminal {
        terminal: crate::models::AgentAssetResolutionTerminal,
        cause: AgentAssetTerminalCauseDraft,
        evidence: Vec<AgentAssetStateEvidenceRefDraft>,
        input: Box<Self>,
    },
    Shadowed {
        winner: AgentAssetNativeRef,
        input: Box<Self>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct AgentAssetResolutionDraft {
    pub relation: AgentAssetResolutionRelation,
    pub qualified_collision: bool,
    pub terminal: Option<crate::models::AgentAssetResolutionTerminal>,
    pub winner: Option<AgentAssetNativeRef>,
    pub control_source: Option<AgentAssetPolicyReferenceDraft>,
}

#[derive(Debug, Clone)]
pub(crate) enum AgentAssetPolicyReferenceDraft {
    Declaration { declaration_id: String },
    Source { source_key: String },
}

pub(crate) type AgentAssetResolver =
    for<'a> fn(AgentAssetResolveRequest<'a>, &mut dyn AgentResolveOutput);

pub(crate) type AgentReadonlyExternalRoots = fn(&[AgentInstallation]) -> Vec<PathBuf>;

#[derive(Clone, Copy)]
pub(crate) struct EnvironmentAdapter {
    discover_contexts: AgentContextDiscovery,
    discover_sources: AgentSourceDiscovery,
    discover_follow_up_sources: Option<AgentFollowUpSourceDiscovery>,
    package_name: &'static str,
    parse: AgentAssetParser,
    resolve: AgentAssetResolver,
    state_assessor: AgentAssetStateAssessor,
    workspace_trust_sources: Option<AgentWorkspaceTrustSourceDiscovery>,
    workspace_trust_resolver: Option<AgentWorkspaceTrustResolver>,
    source_preview:
        Option<fn(&AgentAssetSourceSpec) -> super::environment::preview::AgentSourcePreviewPolicy>,
    mechanism_catalog: Option<super::environment::mutation::AgentAssetMechanismCatalog>,
    mutation_preparer: Option<super::environment::mutation::AgentAssetMutationPreparer>,
    native_unavailable: Option<super::environment::mutation::AgentAssetNativeUnavailable>,
    macos_vendor_signature: Option<(&'static str, &'static str)>,
    readonly_external_roots: Option<AgentReadonlyExternalRoots>,
    definition_selector: Option<AgentDefinitionSelector>,
    catalog_adapter: Option<&'static super::catalog::native::NativeCatalogAdapter>,
    hook_adapter: Option<&'static super::catalog::native::hooks::NativeHookAdapter>,
}

impl EnvironmentAdapter {
    pub(crate) const fn with_pipeline(
        discover_contexts: AgentContextDiscovery,
        discover_sources: AgentSourceDiscovery,
        discover_follow_up_sources: Option<AgentFollowUpSourceDiscovery>,
        package_name: &'static str,
        parse: AgentAssetParser,
        resolve: AgentAssetResolver,
        state_assessor: AgentAssetStateAssessor,
    ) -> Self {
        Self {
            discover_contexts,
            discover_sources,
            discover_follow_up_sources,
            package_name,
            parse,
            resolve,
            state_assessor,
            workspace_trust_sources: None,
            workspace_trust_resolver: None,
            source_preview: None,
            mechanism_catalog: None,
            mutation_preparer: None,
            native_unavailable: None,
            macos_vendor_signature: None,
            readonly_external_roots: None,
            definition_selector: None,
            catalog_adapter: None,
            hook_adapter: None,
        }
    }

    pub(crate) const fn with_catalog_adapter(
        mut self,
        adapter: &'static super::catalog::native::NativeCatalogAdapter,
    ) -> Self {
        self.catalog_adapter = Some(adapter);
        self
    }

    pub(crate) const fn catalog_adapter(
        &self,
    ) -> Option<&'static super::catalog::native::NativeCatalogAdapter> {
        self.catalog_adapter
    }

    pub(crate) const fn with_hook_adapter(
        mut self,
        adapter: &'static super::catalog::native::hooks::NativeHookAdapter,
    ) -> Self {
        self.hook_adapter = Some(adapter);
        self
    }

    pub(crate) const fn hook_adapter(
        &self,
    ) -> Option<&'static super::catalog::native::hooks::NativeHookAdapter> {
        self.hook_adapter
    }

    #[cfg(test)]
    pub(crate) fn with_test_contexts(mut self, discover_contexts: AgentContextDiscovery) -> Self {
        self.discover_contexts = discover_contexts;
        self
    }

    pub(crate) const fn state_assessor(&self) -> AgentAssetStateAssessor {
        self.state_assessor
    }

    pub(crate) const fn with_definition_selector(
        mut self,
        selector: AgentDefinitionSelector,
    ) -> Self {
        self.definition_selector = Some(selector);
        self
    }

    pub(crate) fn definition_suppressions(
        &self,
        request: AgentDefinitionSelectionRequest<'_>,
    ) -> Vec<AgentDefinitionSuppression> {
        self.definition_selector
            .map_or_else(Vec::new, |selector| selector(request))
    }

    pub(crate) const fn with_readonly_external_roots(
        mut self,
        roots: AgentReadonlyExternalRoots,
    ) -> Self {
        self.readonly_external_roots = Some(roots);
        self
    }

    pub(crate) fn readonly_external_roots(
        &self,
        installations: &[AgentInstallation],
    ) -> Vec<PathBuf> {
        self.readonly_external_roots
            .map_or_else(Vec::new, |roots| roots(installations))
    }

    pub(crate) const fn with_source_preview(
        mut self,
        preview: fn(&AgentAssetSourceSpec) -> super::environment::preview::AgentSourcePreviewPolicy,
    ) -> Self {
        self.source_preview = Some(preview);
        self
    }

    pub(crate) const fn with_macos_vendor_signature(
        mut self,
        team_id: &'static str,
        identifier: &'static str,
    ) -> Self {
        self.macos_vendor_signature = Some((team_id, identifier));
        self
    }

    pub(crate) const fn macos_vendor_signature(&self) -> Option<(&'static str, &'static str)> {
        self.macos_vendor_signature
    }

    pub(crate) fn source_preview(
        &self,
        source: &AgentAssetSourceSpec,
    ) -> super::environment::preview::AgentSourcePreviewPolicy {
        self.source_preview.map_or_else(
            super::environment::preview::AgentSourcePreviewPolicy::metadata_only,
            |preview| preview(source),
        )
    }

    pub(crate) const fn with_asset_mutation(
        mut self,
        catalog: super::environment::mutation::AgentAssetMechanismCatalog,
        prepare: super::environment::mutation::AgentAssetMutationPreparer,
        unavailable: super::environment::mutation::AgentAssetNativeUnavailable,
    ) -> Self {
        self.mechanism_catalog = Some(catalog);
        self.mutation_preparer = Some(prepare);
        self.native_unavailable = Some(unavailable);
        self
    }

    pub(crate) fn mechanism_records(
        &self,
        kind: AgentCliKind,
    ) -> Vec<crate::models::AgentAssetMechanismRecord> {
        self.mechanism_catalog
            .map_or_else(Vec::new, |catalog| catalog(kind))
    }

    pub(crate) fn prepare_mutation(
        &self,
        request: super::environment::mutation::MutationPreparation<'_>,
    ) -> Result<
        super::environment::mutation::PreparedMutation,
        crate::models::AgentAssetMutationError,
    > {
        self.mutation_preparer.ok_or_else(|| {
            crate::models::AgentAssetMutationError::unavailable(
                self.native_unavailable_reason(request.asset.category, request.action),
            )
        })?(request)
    }

    pub(crate) fn native_unavailable_reason(
        &self,
        category: AgentAssetCategory,
        action: crate::models::AgentAssetActionKind,
    ) -> crate::models::AgentAssetActionUnavailableReason {
        self.native_unavailable.map_or(
            crate::models::AgentAssetActionUnavailableReason::NoOfficialMechanism,
            |reason| reason(category, action),
        )
    }

    pub(crate) const fn with_workspace_trust_authority(
        mut self,
        discover: AgentWorkspaceTrustSourceDiscovery,
        resolve: AgentWorkspaceTrustResolver,
    ) -> Self {
        self.workspace_trust_sources = Some(discover);
        self.workspace_trust_resolver = Some(resolve);
        self
    }

    pub(crate) const fn package_name(&self) -> &'static str {
        self.package_name
    }

    pub(crate) fn parse(
        &self,
        request: AgentAssetParseRequest<'_>,
        output: &mut dyn AgentParseOutput,
    ) {
        (self.parse)(request, output)
    }

    pub(crate) fn discover_contexts(
        &self,
        request: AgentContextDiscoveryRequest<'_>,
        output: &mut dyn AgentDiagnosticOutput,
    ) -> Vec<AgentConfigurationContext> {
        (self.discover_contexts)(request, output)
    }

    pub(crate) fn discover_sources(
        &self,
        request: AgentSourceDiscoveryRequest<'_>,
        output: &mut dyn InitialSourceOutput,
    ) {
        (self.discover_sources)(request, output)
    }

    pub(crate) fn discover_follow_up_sources(
        &self,
        request: AgentFollowUpSourceDiscoveryRequest<'_>,
        output: &mut dyn FollowUpSourceOutput,
    ) {
        if let Some(discover) = self.discover_follow_up_sources {
            discover(request, output)
        }
    }

    pub(crate) const fn has_follow_up_sources(&self) -> bool {
        self.discover_follow_up_sources.is_some()
    }

    pub(crate) fn resolve(
        &self,
        request: AgentAssetResolveRequest<'_>,
        output: &mut dyn AgentResolveOutput,
    ) {
        (self.resolve)(request, output)
    }

    pub(crate) fn has_workspace_trust_authority(&self) -> bool {
        self.workspace_trust_sources.is_some() && self.workspace_trust_resolver.is_some()
    }

    pub(crate) fn discover_workspace_trust_sources(
        &self,
        request: AgentWorkspaceTrustSourceRequest<'_>,
        output: &mut dyn InitialSourceOutput,
    ) {
        if let Some(discover) = self.workspace_trust_sources {
            discover(request, output);
        }
    }

    pub(crate) fn resolve_workspace_trust(
        &self,
        request: AgentWorkspaceTrustResolveRequest<'_>,
        output: &mut dyn AgentDiagnosticOutput,
    ) -> AgentTrustState {
        self.workspace_trust_resolver
            .map(|resolve| resolve(request, output))
            .unwrap_or(AgentTrustState::Unknown)
    }
}
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvironmentPatch {
    set: BTreeMap<String, String>,
    remove: BTreeSet<String>,
}

impl EnvironmentPatch {
    pub(crate) fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.remove.remove(&name);
        self.set.insert(name, value.into());
    }

    pub(crate) fn remove(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.set.remove(&name);
        self.remove.insert(name);
    }

    pub(crate) fn set_values(&self) -> impl Iterator<Item = (&str, &str)> {
        self.set
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub(crate) fn removed_names(&self) -> impl Iterator<Item = &str> {
        self.remove.iter().map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TemporaryLaunchConfiguration<'a> {
    Native,
    Provider {
        provider_name: &'a str,
        api_key: &'a str,
        base_url: &'a str,
    },
}

pub(crate) struct TemporaryLaunchRequest<'a> {
    pub configuration: TemporaryLaunchConfiguration<'a>,
    pub model: &'a str,
    pub session_name: &'a str,
    pub resume_id: &'a str,
    pub session_mode: TemporaryCliSessionMode,
    pub auxiliary_file_path: Option<&'a Path>,
}

#[derive(Debug, Clone)]
pub(crate) struct TemporaryLaunchPlan {
    pub args: Vec<String>,
    pub environment: EnvironmentPatch,
    pub auxiliary_file_content: Option<String>,
}

type TemporaryLaunchBuilder =
    for<'a> fn(TemporaryLaunchRequest<'a>) -> Result<TemporaryLaunchPlan, String>;

#[derive(Clone, Copy)]
pub(crate) struct TemporaryLaunchFeatures {
    pub model_selection: bool,
    pub session_resume: bool,
    pub session_name: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct TemporaryLaunchAdapter {
    features: TemporaryLaunchFeatures,
    auxiliary_file_name: Option<&'static str>,
    build_plan: TemporaryLaunchBuilder,
}

impl TemporaryLaunchAdapter {
    pub(crate) const fn new(
        features: TemporaryLaunchFeatures,
        auxiliary_file_name: Option<&'static str>,
        build_plan: TemporaryLaunchBuilder,
    ) -> Self {
        Self {
            features,
            auxiliary_file_name,
            build_plan,
        }
    }

    pub(crate) const fn supports_model_selection(&self) -> bool {
        self.features.model_selection
    }

    pub(crate) const fn supports_session_resume(&self) -> bool {
        self.features.session_resume
    }

    pub(crate) const fn supports_session_name(&self) -> bool {
        self.features.session_name
    }

    pub(crate) const fn auxiliary_file_name(&self) -> Option<&'static str> {
        self.auxiliary_file_name
    }

    pub(crate) fn build_plan(
        &self,
        request: TemporaryLaunchRequest<'_>,
    ) -> Result<TemporaryLaunchPlan, String> {
        (self.build_plan)(request)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentFilePlan {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone)]
pub(crate) enum LivenessResponseSource {
    Stdout,
    File(PathBuf),
}

pub(crate) struct LivenessRequest<'a> {
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub model: &'a str,
    pub prompt: &'a str,
    pub timeout_seconds: u64,
    pub isolated_home: &'a Path,
    pub output_path: &'a Path,
}

#[derive(Debug, Clone)]
pub(crate) struct LivenessPlan {
    pub args: Vec<String>,
    pub environment: EnvironmentPatch,
    pub files: Vec<AgentFilePlan>,
    pub response_source: LivenessResponseSource,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedTokenUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedLivenessOutput {
    pub response: String,
    pub error: Option<String>,
    pub usage: ParsedTokenUsage,
}

type LivenessPlanBuilder = for<'a> fn(LivenessRequest<'a>) -> Result<LivenessPlan, String>;
type LivenessOutputParser = fn(&str, &str) -> ParsedLivenessOutput;

#[derive(Clone, Copy)]
pub(crate) struct LivenessAdapter {
    build_plan: LivenessPlanBuilder,
    parse_output: LivenessOutputParser,
}

impl LivenessAdapter {
    pub(crate) const fn new(
        build_plan: LivenessPlanBuilder,
        parse_output: LivenessOutputParser,
    ) -> Self {
        Self {
            build_plan,
            parse_output,
        }
    }

    pub(crate) fn build_plan(&self, request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
        (self.build_plan)(request)
    }

    pub(crate) fn parse_output(&self, response_output: &str, stdout: &str) -> ParsedLivenessOutput {
        (self.parse_output)(response_output, stdout)
    }
}

/// A resolved native library, independent of CLI installation availability.
#[derive(Debug, Clone)]
pub(crate) struct SessionHistorySource {
    pub config_root: PathBuf,
    pub launch_environment: EnvironmentPatch,
    pub resume_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionHistoryRecord {
    pub summary: CliSessionSummary,
    /// Exact private native record locator. Never accepted from the frontend.
    pub locator: PathBuf,
    pub record_key: String,
    pub role: crate::models::AgentSessionRole,
    pub parent_native_id: Option<String>,
    pub resume_reason: Option<String>,
    /// Native metadata is readable, but no validated transcript can be opened.
    /// Such records remain listable; their locator must never be used for reads.
    pub content_unavailable_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionHistoryWorkspace {
    pub workdir: PathBuf,
    pub records: Vec<SessionHistoryRecord>,
    pub complete: bool,
    pub diagnostics: Vec<String>,
}

/// Identity enumeration has no dependency on transcript contents, display
/// summaries or resume eligibility. Both modes use the same native scope and
/// record-key rules; identity records never enter the resume-reference registry.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHistoryReadMode {
    Identities,
    Summaries,
}

impl SessionHistoryRecord {
    pub(crate) fn identity(
        kind: AgentCliKind,
        id: String,
        workdir: &Path,
        locator: PathBuf,
    ) -> Self {
        Self {
            record_key: id.clone(),
            summary: CliSessionSummary {
                id,
                title: String::new(),
                preview: None,
                model: None,
                models: Vec::new(),
                cli_kind: kind,
                created_at: None,
                updated_at: None,
                workdir: workdir.to_string_lossy().into_owned(),
                cli_version: None,
                archived: false,
                can_resume: false,
                metadata_source: "nativeIdentity".into(),
            },
            locator,
            role: crate::models::AgentSessionRole::Unknown,
            parent_native_id: None,
            resume_reason: None,
            content_unavailable_reason: None,
        }
    }
}

impl SessionHistoryWorkspace {
    pub(crate) fn new(workdir: &Path) -> Self {
        Self {
            workdir: workdir.to_path_buf(),
            records: Vec::new(),
            complete: true,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn failed(&mut self, message: impl Into<String>) {
        self.complete = false;
        if self.diagnostics.len() < 4 {
            self.diagnostics.push(message.into());
        }
    }
}

/// A request-owned budget. It is shared with native file readers, so cancelling
/// IPC work also stops enumeration, transcript reads and SQLite row iteration.
#[derive(Debug, Clone)]
pub(crate) struct SessionReadBudget {
    pub deadline: Instant,
    pub cancelled: Arc<AtomicBool>,
    pub max_bytes: u64,
    pub bytes: Arc<std::sync::atomic::AtomicU64>,
}

impl SessionReadBudget {
    pub(crate) fn new(deadline: Instant, cancelled: Arc<AtomicBool>, max_bytes: u64) -> Self {
        Self {
            deadline,
            cancelled,
            max_bytes,
            bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("会话读取已取消".into());
        }
        if Instant::now() >= self.deadline {
            return Err("会话读取达到时间预算；当前结果不完整".into());
        }
        if self.bytes.load(Ordering::Relaxed) > self.max_bytes {
            return Err("会话读取达到字节预算；当前结果不完整".into());
        }
        Ok(())
    }

    pub(crate) fn record(&self, bytes: usize) -> Result<(), String> {
        self.bytes.fetch_add(bytes as u64, Ordering::Relaxed);
        self.check()
    }
}

type SessionHistorySourceResolver = fn() -> Result<SessionHistorySource, String>;
type SessionHistoryScanner = fn(
    AgentCliKind,
    &Path,
    &[PathBuf],
    &SessionReadBudget,
    SessionHistoryReadMode,
) -> Result<Vec<SessionHistoryWorkspace>, String>;
type SessionHistoryDetailReader =
    fn(&SessionHistoryRecord, SessionReadLimits) -> Result<CliSessionDetail, String>;
type SessionHistorySearcher = fn(
    &SessionHistoryRecord,
    &SessionContentSearchRequest,
    &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String>;
type SessionHistoryIndexReader = fn(
    &SessionHistoryRecord,
    Option<&str>,
    &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String>;

#[derive(Clone, Copy)]
pub(crate) struct SessionHistoryAdapter {
    pub source: SessionHistorySourceResolver,
    pub scan: SessionHistoryScanner,
    pub detail: SessionHistoryDetailReader,
    pub search: SessionHistorySearcher,
    pub index: SessionHistoryIndexReader,
}

pub(crate) type SessionMetadataLookup =
    for<'a> fn(
        SessionMetadataLookupRequest<'a>,
    ) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError>;

#[derive(Debug, Clone)]
pub(crate) struct SessionMetadataLookupRequest<'a> {
    pub cli_kind: AgentCliKind,
    pub session_id: &'a str,
    pub workdir: Option<&'a Path>,
    pub transcript_path_hint: Option<&'a Path>,
    pub previous: Option<&'a SessionMetadataCursor>,
    pub budget: SessionMetadataLookupBudget,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionMetadataLookupBudget {
    pub max_bytes: usize,
    pub deadline: Instant,
    pub cancelled: Arc<AtomicBool>,
}

impl SessionMetadataLookupBudget {
    pub(crate) fn check(&self, bytes: usize) -> Result<(), SessionMetadataLookupError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(SessionMetadataLookupError::Cancelled);
        }
        if Instant::now() >= self.deadline || bytes > self.max_bytes {
            return Err(SessionMetadataLookupError::TimedOut);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMetadataSnapshot {
    pub title: Option<String>,
    pub model: Option<String>,
    pub workdir: Option<String>,
    pub last_activity_at: Option<i64>,
    pub source_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMetadataCursor {
    pub source_identity: String,
    pub source_len: u64,
    pub next_offset: u64,
    pub parser_version: u32,
    pub opaque_state: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionMetadataLookupResult {
    Ready {
        snapshot: SessionMetadataSnapshot,
        cursor: Option<SessionMetadataCursor>,
    },
    Pending {
        partial: Option<SessionMetadataSnapshot>,
        cursor: SessionMetadataCursor,
    },
    NotReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionMetadataLookupError {
    Unsupported(String),
    Cancelled,
    TimedOut,
    InvalidSource,
    Io(String),
    Parse(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionSearchTerm {
    pub index: usize,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionContentSearchRequest {
    pub terms: Vec<SessionSearchTerm>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionContentSearchResult {
    pub matched_term_indexes: Vec<usize>,
    pub has_content: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionReadLimits {
    pub max_file_bytes: usize,
    pub max_messages: usize,
    pub max_total_chars: usize,
    pub max_message_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionIndexMessage {
    pub id: String,
    pub role: CliSessionMessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionIndexLoadResult {
    Unchanged {
        fingerprint: String,
        source_bytes: u64,
    },
    Updated {
        fingerprint: String,
        source_bytes: u64,
        messages: Vec<SessionIndexMessage>,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct SessionAdapter {
    history: SessionHistoryAdapter,
    metadata: Option<SessionMetadataLookup>,
}

impl SessionAdapter {
    pub(crate) const fn new(
        history: SessionHistoryAdapter,
        metadata: Option<SessionMetadataLookup>,
    ) -> Self {
        Self { history, metadata }
    }
    pub(crate) const fn supports_detail(&self) -> bool {
        true
    }
    pub(crate) const fn supports_search(&self) -> bool {
        true
    }
    pub(crate) const fn supports_metadata(&self) -> bool {
        self.metadata.is_some()
    }
    pub(crate) fn history(&self) -> &SessionHistoryAdapter {
        &self.history
    }
    pub(crate) fn lookup_metadata(
        &self,
        request: SessionMetadataLookupRequest<'_>,
    ) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
        let lookup = self.metadata.ok_or_else(|| {
            SessionMetadataLookupError::Unsupported(
                "当前 Agent CLI 不支持定向会话元数据读取".into(),
            )
        })?;
        lookup(request)
    }
}

type ConfigSnapshotReader = fn(AgentCliKind, &[Provider]) -> CliConfigSnapshot;
type ConfigCandidateBuilder =
    fn(
        AgentCliKind,
        &Provider,
        &str,
    ) -> Result<Vec<super::configuration::contracts::ProviderConfigurationCandidate>, String>;

#[derive(Clone, Copy)]
pub(crate) struct DefaultConfigAdapter {
    snapshot: ConfigSnapshotReader,
    candidates: ConfigCandidateBuilder,
}

impl DefaultConfigAdapter {
    pub(crate) const fn new(
        snapshot: ConfigSnapshotReader,
        candidates: ConfigCandidateBuilder,
    ) -> Self {
        Self {
            snapshot,
            candidates,
        }
    }

    pub(crate) fn snapshot(
        &self,
        cli_kind: AgentCliKind,
        providers: &[Provider],
    ) -> CliConfigSnapshot {
        (self.snapshot)(cli_kind, providers)
    }

    pub(crate) fn candidates(
        &self,
        cli_kind: AgentCliKind,
        provider: &Provider,
        api_key_local_id: &str,
    ) -> Result<Vec<super::configuration::contracts::ProviderConfigurationCandidate>, String> {
        (self.candidates)(cli_kind, provider, api_key_local_id)
    }
}

type EndpointNormalizer = fn(&str) -> String;

#[derive(Clone, Copy)]
pub(crate) struct EndpointAdapter {
    normalize_base_url: EndpointNormalizer,
}

impl EndpointAdapter {
    pub(crate) const fn new(normalize_base_url: EndpointNormalizer) -> Self {
        Self { normalize_base_url }
    }

    pub(crate) fn normalize_base_url(&self, base_url: &str) -> String {
        (self.normalize_base_url)(base_url)
    }
}
