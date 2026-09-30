//! Asset identities and desired definitions are separate from native inventory.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogOwnership {
    Observed,
    Managed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogDrift {
    Observed,
    InSync,
    UpdateAvailable,
    Modified,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogVariant {
    pub id: String,
    pub label: String,
    pub complete: bool,
    pub summary: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentRequest {
    pub asset_id: String,
    pub workspace: Option<String>,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentFile {
    pub key: String,
    /// Physical file and selected entry, shared across equivalent bindings.
    pub target_id: String,
    pub document_id: Option<String>,
    pub path: Option<String>,
    pub read_only_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentSource {
    /// None identifies a shared definition, which has no native file target.
    pub binding_id: Option<String>,
    pub agent_kind: Option<AgentCliKind>,
    pub scope: Option<AgentAssetScope>,
    pub label: Option<String>,
    pub path: Option<String>,
    pub files: Vec<AgentCatalogContentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentDocument {
    /// Content role, independent of an Agent's native filename.
    pub key: String,
    pub label: String,
    pub format: AgentConfigurationFormat,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentDocumentChange {
    pub key: String,
    /// Missing content in an incomplete read must not appear as a deletion.
    pub comparable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentFactChange {
    pub label: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentComparison {
    pub documents: Vec<AgentCatalogContentDocumentChange>,
    pub facts: Vec<AgentCatalogContentFactChange>,
    pub description_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentGroup {
    pub id: String,
    pub description: Option<String>,
    pub facts: Vec<AgentResourceContentFact>,
    pub documents: Vec<AgentCatalogContentDocument>,
    pub sources: Vec<AgentCatalogContentSource>,
    pub complete: bool,
    pub notes: Vec<String>,
    /// Differences from the first group, determined by Rust from full reads.
    pub comparison: Option<AgentCatalogContentComparison>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContentUnavailable {
    pub source: AgentCatalogContentSource,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogContent {
    pub asset_id: String,
    pub category: AgentAssetCategory,
    pub shared_version: Option<u64>,
    pub groups: Vec<AgentCatalogContentGroup>,
    pub unavailable: Vec<AgentCatalogContentUnavailable>,
    pub pending_sources: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogBinding {
    pub id: String,
    pub usage: Option<AgentCatalogBindingUsage>,
    pub native: AgentAssetRecord,
    /// Catalog operations can also act on persisted suspended targets. These
    /// actions are not routed through the direct native MutationService API.
    pub actions: Vec<AgentAssetAction>,
    pub variant_id: Option<String>,
    pub drift: AgentCatalogDrift,
    pub applied_version: Option<u64>,
    pub can_adopt: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogBindingUsage {
    pub group_id: String,
    pub primary_binding_id: String,
    pub state: AgentAssetState,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogAsset {
    pub id: String,
    pub content_revision: String,
    pub name: String,
    pub category: AgentAssetCategory,
    /// Source-file timestamps, never the time of the inventory scan. Shared
    /// files share their timestamps; unavailable metadata remains unknown.
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub hook: Option<AgentCatalogHookSummary>,
    pub ownership: AgentCatalogOwnership,
    pub provenance: super::AgentAssetProvenanceSummary,
    pub version: Option<u64>,
    pub variants: Vec<AgentCatalogVariant>,
    pub bindings: Vec<AgentCatalogBinding>,
    pub unresolved_targets: Vec<AgentCatalogUnresolvedTarget>,
    pub candidate_ids: Vec<String>,
    pub separated_asset_ids: Vec<String>,
    pub manual_associations: Vec<AgentCatalogManualAssociationSummary>,
    pub application: AgentCatalogApplication,
    pub definition_removal: AgentCatalogDefinitionRemoval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogHookRule {
    pub name: String,
    pub event: String,
    pub matcher: Option<String>,
    pub execution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogHookSource {
    pub binding_id: String,
    pub label: String,
    pub parent_native_asset_id: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogHookSummary {
    pub rules: Vec<AgentCatalogHookRule>,
    pub sources: Vec<AgentCatalogHookSource>,
}

impl AgentCatalogAsset {
    pub(crate) fn hook_source(&self, binding_id: &str) -> Option<&AgentCatalogHookSource> {
        self.hook
            .as_ref()?
            .sources
            .iter()
            .find(|source| source.binding_id == binding_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogDefinitionRemoval {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogDeleteRequest {
    pub asset_id: String,
    pub expected_version: u64,
    pub expected_revision: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogApplication {
    /// A complete, unambiguous definition is available for preparing a plan.
    /// Individual destinations retain their own native write gates.
    pub available: bool,
    /// Observed definitions use this exact binding for a read-only plan. A
    /// confirmed operation adopts it before native application.
    pub source_binding_id: Option<String>,
    pub targets: Vec<AgentCatalogTarget>,
    pub observations: Vec<AgentCatalogAgentObservation>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogObservationState {
    Observed,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogAgentObservation {
    pub agent_kind: AgentCliKind,
    pub state: AgentCatalogObservationState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogUnresolvedTarget {
    pub target_id: String,
    pub context_id: String,
    pub agent_kind: AgentCliKind,
    pub scope: AgentAssetScope,
    pub drift: AgentCatalogDrift,
    pub applied_version: u64,
    pub message: String,
    pub state: AgentCatalogUnresolvedState,
    pub actions: Vec<AgentAssetAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogUnresolvedState {
    Missing,
    Unknown,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogTarget {
    pub id: String,
    pub context_id: String,
    pub agent_kind: AgentCliKind,
    pub scope: AgentAssetScope,
    pub label: String,
    pub categories: Vec<AgentAssetCategory>,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogCounts {
    pub skill: usize,
    pub mcp: usize,
    pub extension: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssetCatalog {
    pub revision: String,
    pub counts: BTreeMap<AgentCliKind, AgentCatalogCounts>,
    pub creatable_categories: Vec<AgentAssetCategory>,
    pub inventory: AgentEnvironmentInventory,
    pub assets: Vec<AgentCatalogAsset>,
    pub targets: Vec<AgentCatalogTarget>,
    pub diagnostics: Vec<String>,
}

/// Editable shared MCP JSON. Missing type is inferred from command or URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogMcpInput {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<AgentMcpTransport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(rename = "env", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub connection_options: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpFormRule {
    pub key: String,
    pub local: bool,
    pub remote: bool,
    pub supported: bool,
    pub support_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMcpFormRead {
    pub input: AgentCatalogMcpInput,
    pub fields: Vec<AgentMcpFormRule>,
    pub transports: Vec<AgentMcpFormRule>,
    pub target_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogHookVariantInput {
    pub agent_kind: AgentCliKind,
    pub event: String,
    pub group_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogHookInput {
    pub variants: Vec<AgentCatalogHookVariantInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogDefinition {
    pub asset_id: String,
    pub name: String,
    pub category: AgentAssetCategory,
    pub version: u64,
    pub mcp: Option<AgentCatalogMcpInput>,
    pub hook: Option<AgentCatalogHookInput>,
    /// Original local library text.
    /// Omitted content preserves it and every bundled resource.
    pub skill_markdown: Option<String>,
    pub files: Vec<AgentCatalogPackageFile>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogPackageFile {
    pub path: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogSaveRequest {
    pub asset_id: Option<String>,
    pub expected_version: Option<u64>,
    pub name: String,
    pub category: AgentAssetCategory,
    pub mcp: Option<AgentCatalogMcpInput>,
    pub hook: Option<AgentCatalogHookInput>,
    pub skill_markdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogAdoptRequest {
    pub binding_id: String,
    pub expected_revision: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogAction {
    ApplyDefinition,
    Enable,
    Disable,
    RemoveBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgentCatalogPlanSource {
    Catalog {
        asset_id: String,
        expected_version: Option<u64>,
    },
    NativeBinding {
        asset_id: String,
        binding_id: String,
    },
    Draft {
        definition: Box<AgentCatalogSaveRequest>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogPlanRequest {
    pub source: AgentCatalogPlanSource,
    pub action: AgentCatalogAction,
    /// Apply: opaque destination/member/receipt IDs. Enable, disable and remove:
    /// current native binding IDs or persisted suspended-target IDs.
    pub target_ids: Vec<String>,
    pub expected_revision: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogDefinitionChangeKind {
    Adopt,
    Save,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogDefinitionChange {
    pub kind: AgentCatalogDefinitionChangeKind,
    pub name: String,
    pub before_version: Option<u64>,
    pub after_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogTargetKind {
    Destination,
    Binding,
    Retained,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogTargetPlan {
    pub target_id: String,
    pub label: String,
    pub agent_kind: AgentCliKind,
    pub context_id: String,
    pub scope: AgentAssetScope,
    pub target_kind: AgentCatalogTargetKind,
    pub changes: Vec<AgentAssetPlanChange>,
    pub affected_asset_ids: Vec<String>,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogConfigurationState {
    Missing,
    Current,
    Different,
    Unknown,
}

/// Backend-selected destinations for Skill and MCP configuration. Physical
/// destination IDs remain the sole input to the existing preview/commit path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogConfigurationChoice {
    pub id: String,
    pub label: String,
    pub agent_kinds: Vec<AgentCliKind>,
    pub scope: AgentAssetScope,
    pub shared: bool,
    pub shared_choice_id: Option<String>,
    pub target_ids: Vec<String>,
    pub related_asset_ids: Vec<String>,
    pub state: AgentCatalogConfigurationState,
    pub available: bool,
    pub detail: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogConfigurationSelection {
    pub choices: Vec<AgentCatalogConfigurationChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogPlan {
    pub token: Option<String>,
    pub plan_id: Option<String>,
    pub asset_id: String,
    pub action: AgentCatalogAction,
    pub version: Option<u64>,
    pub expires_at: String,
    pub targets: Vec<AgentCatalogTargetPlan>,
    pub notes: Vec<String>,
    pub definition_change: Option<AgentCatalogDefinitionChange>,
    pub selection: Option<AgentCatalogConfigurationSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogApplyRequest {
    pub plan_token: String,
    pub asset_id: String,
    pub action: AgentCatalogAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogTargetResult {
    pub target_id: String,
    pub label: String,
    pub phase: AgentAssetOperationPhase,
    pub outcome: Option<AgentAssetOperationOutcome>,
    pub message: Option<String>,
    pub native_operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogOperation {
    pub id: String,
    pub plan_id: String,
    pub asset_id: String,
    pub action: AgentCatalogAction,
    pub phase: AgentAssetOperationPhase,
    pub revision: u64,
    pub can_cancel: bool,
    pub created_at: String,
    pub updated_at: String,
    pub targets: Vec<AgentCatalogTargetResult>,
    pub definition_change: Option<AgentCatalogDefinitionChangeResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogDefinitionChangeState {
    Pending,
    Saved,
    Unchanged,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogDefinitionChangeResult {
    pub kind: AgentCatalogDefinitionChangeKind,
    pub state: AgentCatalogDefinitionChangeState,
    pub version: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgentCatalogRelationIntent {
    Compare {
        left_asset_id: String,
        right_asset_id: String,
    },
    Merge {
        destination_asset_id: String,
        source_asset_id: String,
    },
    KeepSeparate {
        left_asset_id: String,
        right_asset_id: String,
        hide_candidate: bool,
    },
    Detach {
        association_id: String,
    },
    RestoreHint {
        left_asset_id: String,
        right_asset_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogRelationAction {
    Compare,
    Merge,
    KeepSeparate,
    Detach,
    RestoreHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogRelationPreviewRequest {
    pub intent: AgentCatalogRelationIntent,
    pub expected_revision: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogComparisonFormat {
    Markdown,
    Json,
    Text,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogComparisonDocument {
    pub key: String,
    pub label: String,
    pub path: Option<String>,
    pub format: AgentCatalogComparisonFormat,
    pub content: Option<String>,
    pub truncated: bool,
    pub executable: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogComparisonBinding {
    pub binding_id: String,
    pub agent_kind: AgentCliKind,
    pub context_id: String,
    pub scope: AgentAssetScope,
    pub path: Option<String>,
    pub provenance: Vec<AgentAssetProvenance>,
    pub complete: bool,
    pub reason: Option<String>,
    pub documents: Vec<AgentCatalogComparisonDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogComparisonSide {
    pub asset_id: String,
    pub name: String,
    pub category: AgentAssetCategory,
    pub ownership: AgentCatalogOwnership,
    pub version: Option<u64>,
    pub complete: bool,
    pub reason: Option<String>,
    pub definition: Vec<AgentCatalogComparisonDocument>,
    pub bindings: Vec<AgentCatalogComparisonBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogComparisonEquality {
    Equal,
    Different,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCatalogDifferenceKind {
    Added,
    Removed,
    Changed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogDifference {
    pub path: String,
    pub kind: AgentCatalogDifferenceKind,
    pub left_summary: Option<String>,
    pub right_summary: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogRelationCapability {
    pub intent: AgentCatalogRelationIntent,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogRelationPreview {
    pub token: Option<String>,
    pub relation_key: String,
    pub action: AgentCatalogRelationAction,
    pub expires_at: Option<String>,
    pub sides: Vec<AgentCatalogComparisonSide>,
    pub equality: AgentCatalogComparisonEquality,
    pub differences: Vec<AgentCatalogDifference>,
    pub capabilities: Vec<AgentCatalogRelationCapability>,
    pub affected_binding_ids: Vec<String>,
    pub affected_receipt_target_ids: Vec<String>,
    pub available: bool,
    pub reason: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogRelationCommitRequest {
    pub plan_token: String,
    pub relation_key: String,
    pub action: AgentCatalogRelationAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogRelationCommitResult {
    pub asset_ids: Vec<String>,
    pub association_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogManualAssociationSummary {
    pub id: String,
    pub label: String,
    pub source_asset_ids: Vec<String>,
    pub can_detach: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCatalogAgentPanelRequest {
    pub asset_id: String,
    /// None reads all configurations for the asset detail drawer.
    pub agent_kind: Option<AgentCliKind>,
    pub expected_revision: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogAgentPanel {
    pub asset_id: String,
    pub agent_kind: Option<AgentCliKind>,
    pub revision: String,
    pub observation: Option<AgentCatalogAgentObservation>,
    pub entries: Vec<AgentCatalogAgentPanelEntry>,
    pub sync_source: Option<String>,
    pub batch_actions: Vec<AgentCatalogAgentPanelAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogAgentPanelEntry {
    pub target_id: String,
    pub usage: Option<AgentCatalogBindingUsage>,
    pub target_kind: AgentCatalogTargetKind,
    pub context_id: String,
    pub scope: AgentAssetScope,
    pub label: String,
    pub path: Option<String>,
    pub state_label: String,
    pub sync_state: Option<AgentCatalogConfigurationState>,
    pub reason: Option<String>,
    pub diagnostics: Vec<AgentAssetDiagnostic>,
    pub actions: Vec<AgentCatalogAgentPanelAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogAgentPanelAction {
    pub action: AgentCatalogAction,
    pub label: String,
    pub target_ids: Vec<String>,
    pub available: bool,
    pub reason: Option<String>,
    pub affected_asset_ids: Vec<String>,
    pub parent_native_asset_id: Option<String>,
}
