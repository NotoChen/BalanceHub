import type { AgentConfigurationFormat } from "./agent-configuration-types"
import type {
  AgentAssetCategory, AgentAssetRecord, AgentAssetScope, AgentCliKind,
  AgentEnvironmentInventory, AgentMcpTransport, AgentAssetPlanChange,
  AgentAssetOperationPhase, AgentAssetOperationOutcome, AgentAssetProvenanceSummary,
  AgentAssetAction, AgentAssetProvenance, AgentAssetDiagnostic, AgentAssetState,
} from './provider-types'

export type AgentCatalogOwnership = 'observed' | 'managed'
export type AgentCatalogDrift = 'observed' | 'inSync' | 'updateAvailable' | 'modified' | 'missing' | 'unknown'
export type AgentCatalogAction = 'applyDefinition' | 'enable' | 'disable' | 'removeBinding'
export interface AgentCatalogVariant { id: string; label: string; complete: boolean; summary: string[] }
export interface AgentCatalogContentRequest { assetId: string; workspace: string | null; requestId: string }
export interface AgentCatalogContentFile {
  key: string
  targetId: string
  documentId: string | null
  path: string | null
  readOnlyReason: string | null
}
export interface AgentCatalogContentSource {
  bindingId: string | null
  agentKind: AgentCliKind | null
  scope: AgentAssetScope | null
  label: string | null
  path: string | null
  files: AgentCatalogContentFile[]
}
export interface AgentCatalogContentDocument {
  key: string
  label: string
  format: AgentConfigurationFormat
  text: string
}
export interface AgentCatalogContentGroup {
  id: string
  description: string | null
  facts: Array<{ label: string; value: string }>
  documents: AgentCatalogContentDocument[]
  sources: AgentCatalogContentSource[]
  complete: boolean
  notes: string[]
  comparison: {
    documents: Array<{ key: string; comparable: boolean }>
    facts: Array<{ label: string; before: string | null; after: string | null }>
    descriptionChanged: boolean
  } | null
}
export interface AgentCatalogContent {
  assetId: string
  category: AgentAssetCategory
  sharedVersion: number | null
  groups: AgentCatalogContentGroup[]
  unavailable: Array<{ source: AgentCatalogContentSource; reason: string }>
  pendingSources: number
}
export interface AgentCatalogBinding {
  id: string
  usage: AgentCatalogBindingUsage | null
  native: AgentAssetRecord
  actions: AgentAssetAction[]
  variantId: string | null
  drift: AgentCatalogDrift
  appliedVersion: number | null
  canAdopt: boolean
  reason: string | null
}
export interface AgentCatalogBindingUsage {
  groupId: string
  primaryBindingId: string
  state: AgentAssetState
  detail: string
}
export interface AgentCatalogAsset {
  id: string
  contentRevision: string
  name: string
  category: AgentAssetCategory
  createdAt: string | null
  modifiedAt: string | null
  hook: AgentCatalogHookSummary | null
  ownership: AgentCatalogOwnership
  provenance: AgentAssetProvenanceSummary
  version: number | null
  variants: AgentCatalogVariant[]
  bindings: AgentCatalogBinding[]
  unresolvedTargets: AgentCatalogUnresolvedTarget[]
  candidateIds: string[]
  separatedAssetIds: string[]
  manualAssociations: AgentCatalogManualAssociationSummary[]
  application: AgentCatalogApplication
  definitionRemoval: AgentCatalogDefinitionRemoval
}
export interface AgentCatalogHookRule { name: string; event: string; matcher: string | null; execution: string }
export interface AgentCatalogHookSource { bindingId: string; label: string; parentNativeAssetId: string | null; path: string | null }
export interface AgentCatalogHookSummary { rules: AgentCatalogHookRule[]; sources: AgentCatalogHookSource[] }
export interface AgentCatalogDefinitionRemoval { available: boolean; reason: string | null }
export interface AgentCatalogDeleteRequest { assetId: string; expectedVersion: number; expectedRevision: string; workspace: string | null }
export interface AgentCatalogApplication {
  available: boolean
  sourceBindingId: string | null
  targets: AgentCatalogTarget[]
  observations: AgentCatalogAgentObservation[]
  reason: string | null
}
export interface AgentCatalogAgentObservation {
  agentKind: AgentCliKind
  state: 'observed' | 'missing' | 'unknown'
  reason: string | null
}
export interface AgentCatalogUnresolvedTarget {
  targetId: string
  contextId: string
  agentKind: AgentCliKind
  scope: AgentAssetScope
  drift: AgentCatalogDrift
  appliedVersion: number
  message: string
  state: 'missing' | 'unknown' | 'suspended'
  actions: AgentAssetAction[]
}
export interface AgentCatalogTarget {
  id: string
  contextId: string
  agentKind: AgentCliKind
  scope: AgentAssetScope
  label: string
  categories: AgentAssetCategory[]
  available: boolean
  reason: string | null
}
export interface AgentAssetCatalog {
  revision: string
  counts: Partial<Record<AgentCliKind, Record<'skill' | 'mcp' | 'extension', number>>>
  creatableCategories: AgentAssetCategory[]
  inventory: AgentEnvironmentInventory
  assets: AgentCatalogAsset[]
  targets: AgentCatalogTarget[]
  diagnostics: string[]
}
export interface AgentCatalogMcpInput {
  type?: AgentMcpTransport | 'streamable-http' | 'ws' | 'websocket' | null
  command?: string | null
  args?: string[]
  url?: string | null
  cwd?: string | null
  env?: Record<string, string>
  headers?: Record<string, string>
  connectionOptions?: Record<string, unknown>
}
export interface AgentMcpFormRule {
  key: string
  local: boolean
  remote: boolean
  supported: boolean
  supportLabel: string
}
export interface AgentMcpFormRead {
  input: AgentCatalogMcpInput
  fields: AgentMcpFormRule[]
  transports: AgentMcpFormRule[]
  targetLabel: string | null
}
export interface AgentCatalogHookVariantInput {
  agentKind: AgentCliKind
  event: string
  groupJson: string
}
export interface AgentCatalogHookInput {
  variants: AgentCatalogHookVariantInput[]
}
export interface AgentCatalogDefinition {
  assetId: string
  name: string
  category: AgentAssetCategory
  version: number
  mcp: AgentCatalogMcpInput | null
  hook: AgentCatalogHookInput | null
  skillMarkdown: string | null
  files: Array<{ path: string; sizeBytes: number }>
  notes: string[]
}
export interface AgentCatalogSaveRequest {
  assetId: string | null
  expectedVersion: number | null
  name: string
  category: AgentAssetCategory
  mcp: AgentCatalogMcpInput | null
  hook: AgentCatalogHookInput | null
  skillMarkdown: string | null
}
export interface AgentCatalogAdoptRequest { bindingId: string; expectedRevision: string; workspace: string | null }
export type AgentCatalogPlanSource =
  | { kind: 'catalog'; assetId: string; expectedVersion: number | null }
  | { kind: 'nativeBinding'; assetId: string; bindingId: string }
  | { kind: 'draft'; definition: AgentCatalogSaveRequest }
export interface AgentCatalogPlanRequest {
  source: AgentCatalogPlanSource
  action: AgentCatalogAction
  /** Apply: catalog target IDs. Enable/disable: native binding IDs. */
  targetIds: string[]
  expectedRevision: string
  workspace: string | null
}
export interface AgentCatalogDefinitionChange {
  kind: 'adopt' | 'save'
  name: string
  beforeVersion: number | null
  afterVersion: number
}
export interface AgentCatalogTargetPlan {
  targetId: string
  label: string
  agentKind: AgentCliKind
  contextId: string
  scope: AgentAssetScope
  targetKind: 'destination' | 'binding' | 'retained'
  changes: AgentAssetPlanChange[]
  affectedAssetIds: string[]
  available: boolean
  reason: string | null
}
export interface AgentCatalogConfigurationChoice {
  id: string
  label: string
  agentKinds: AgentCliKind[]
  scope: AgentAssetScope
  shared: boolean
  sharedChoiceId: string | null
  targetIds: string[]
  relatedAssetIds: string[]
  state: 'missing' | 'current' | 'different' | 'unknown'
  available: boolean
  detail: string
  reason: string | null
}
export interface AgentCatalogConfigurationSelection {
  choices: AgentCatalogConfigurationChoice[]
}
export interface AgentCatalogPlan {
  token: string | null
  planId: string | null
  assetId: string
  action: AgentCatalogAction
  version: number | null
  expiresAt: string
  targets: AgentCatalogTargetPlan[]
  notes: string[]
  definitionChange: AgentCatalogDefinitionChange | null
  selection: AgentCatalogConfigurationSelection | null
}
export interface AgentCatalogApplyRequest { planToken: string; assetId: string; action: AgentCatalogAction }
export interface AgentCatalogTargetResult {
  targetId: string
  label: string
  phase: AgentAssetOperationPhase
  outcome: AgentAssetOperationOutcome | null
  message: string | null
  nativeOperationId: string | null
}
export interface AgentCatalogOperation {
  id: string
  planId: string
  assetId: string
  action: AgentCatalogAction
  phase: AgentAssetOperationPhase
  revision: number
  canCancel: boolean
  createdAt: string
  updatedAt: string
  targets: AgentCatalogTargetResult[]
  definitionChange: {
    kind: 'adopt' | 'save'
    state: 'pending' | 'saved' | 'unchanged' | 'unknown'
    version: number | null
    message: string | null
  } | null
}

export type AgentCatalogRelationMutation =
  | { kind: 'merge'; destinationAssetId: string; sourceAssetId: string }
  | { kind: 'keepSeparate'; leftAssetId: string; rightAssetId: string; hideCandidate: boolean }
  | { kind: 'detach'; associationId: string }
  | { kind: 'restoreHint'; leftAssetId: string; rightAssetId: string }
export type AgentCatalogRelationIntent =
  | { kind: 'compare'; leftAssetId: string; rightAssetId: string }
  | AgentCatalogRelationMutation
export interface AgentCatalogRelationPreviewRequest {
  intent: AgentCatalogRelationIntent
  expectedRevision: string
  workspace: string | null
}
export interface AgentCatalogComparisonDocument {
  key: string
  label: string
  path: string | null
  format: 'markdown' | 'json' | 'text' | 'binary'
  content: string | null
  truncated: boolean
  executable: boolean | null
  reason: string | null
}
export interface AgentCatalogComparisonBinding {
  bindingId: string
  agentKind: AgentCliKind
  contextId: string
  scope: AgentAssetScope
  path: string | null
  provenance: AgentAssetProvenance[]
  complete: boolean
  reason: string | null
  documents: AgentCatalogComparisonDocument[]
}
export interface AgentCatalogComparisonSide {
  assetId: string
  name: string
  category: AgentAssetCategory
  ownership: AgentCatalogOwnership
  version: number | null
  complete: boolean
  reason: string | null
  definition: AgentCatalogComparisonDocument[]
  bindings: AgentCatalogComparisonBinding[]
}
export interface AgentCatalogDifference {
  path: string
  kind: 'added' | 'removed' | 'changed' | 'unknown'
  leftSummary: string | null
  rightSummary: string | null
  reason: string | null
}
export interface AgentCatalogRelationCapability {
  intent: AgentCatalogRelationMutation
  available: boolean
  reason: string | null
}
export interface AgentCatalogRelationPreview {
  token: string | null
  relationKey: string
  action: AgentCatalogRelationIntent['kind']
  expiresAt: string | null
  sides: AgentCatalogComparisonSide[]
  equality: 'equal' | 'different' | 'unknown'
  differences: AgentCatalogDifference[]
  capabilities: AgentCatalogRelationCapability[]
  affectedBindingIds: string[]
  affectedReceiptTargetIds: string[]
  available: boolean
  reason: string | null
  notes: string[]
}
export interface AgentCatalogRelationCommitRequest {
  planToken: string
  relationKey: string
  action: AgentCatalogRelationMutation['kind']
}
export interface AgentCatalogRelationCommitResult {
  assetIds: string[]
  associationId: string | null
  message: string
}
export interface AgentCatalogManualAssociationSummary {
  id: string
  label: string
  sourceAssetIds: string[]
  canDetach: boolean
  reason: string | null
}
export interface AgentCatalogAgentPanelRequest {
  assetId: string
  agentKind: AgentCliKind | null
  expectedRevision: string
  workspace: string | null
}
export interface AgentCatalogAgentPanel {
  assetId: string
  agentKind: AgentCliKind | null
  revision: string
  observation: AgentCatalogAgentObservation | null
  syncSource: string | null
  batchActions: AgentCatalogPanelAction[]
  entries: Array<{
    targetId: string
    usage: AgentCatalogBindingUsage | null
    targetKind: 'destination' | 'binding' | 'retained'
    contextId: string
    scope: AgentAssetScope
    label: string
    path: string | null
    stateLabel: string
    syncState: AgentCatalogConfigurationChoice["state"] | null
    reason: string | null
    diagnostics: AgentAssetDiagnostic[]
    actions: AgentCatalogPanelAction[]
  }>
}

export interface AgentCatalogPanelAction {
  action: AgentCatalogAction
  label: string
  targetIds: string[]
  available: boolean
  reason: string | null
  affectedAssetIds: string[]
  parentNativeAssetId: string | null
}
