import type {
  AgentAssetAccess, AgentAssetAccessRisk, AgentAssetCategory, AgentAssetOpenTarget, AgentAssetOperationOutcome,
  AgentAssetOperationPhase, AgentAssetPlanChange, AgentAssetRevision, AgentAssetScope, AgentCliKind,
} from "./provider-types";

// Wire mirror of models/agent_configuration.rs. Rust owns capabilities and validation.
export type AgentConfigurationFormat = "toml" | "json" | "jsonc" | "dotenv" | "markdown";
export type AgentConfigurationActionKind = "read" | "edit" | "create" | "open" | "reveal";
export type AgentConfigurationEffectKind = "contributes" | "overridden" | "inactive" | "unknown";
export type AgentConfigurationDiagnosticSeverity = "info" | "warning" | "error";
export type AgentConfigurationFileState = "unchanged" | "applied" | "unknown";
export type AgentConfigurationErrorKind = "invalidRequest" | "accessExpired" | "actorMismatch" | "sourceChanged" | "rootChanged"
  | "sourceUnavailable" | "readOnly" | "unsupportedFormat" | "invalidSyntax" | "unsupportedScope"
  | "editExpired" | "planExpired" | "planConsumed" | "operationNotFound" | "capacityExceeded"
  | "timeout" | "canceled" | "writeFailed" | "unsupportedPlatform" | "internalFailure";

export interface AgentConfigurationDiagnostic {
  code: string; severity: AgentConfigurationDiagnosticSeverity; message: string;
  sourceId: string | null; line: number | null; column: number | null;
}
export interface AgentConfigurationAction {
  action: AgentConfigurationActionKind; available: boolean; reason: string | null; risks: AgentAssetAccessRisk[];
}
export interface AgentConfigurationEffect {
  kind: AgentConfigurationEffectKind; message: string; relatedSourceIds: string[]; evidence: string[];
}
export interface AgentConfigurationListRequest { agentKind: AgentCliKind; workspace: string | null }
export interface AgentConfigurationSource {
  sourceId: string; contextId: string; environmentId: string; agentKind: AgentCliKind;
  workspace: string | null; nativeRole: string; scope: AgentAssetScope; profile: string | null;
  label: string; path: string; format: AgentConfigurationFormat; revision: AgentAssetRevision;
  access: AgentAssetAccess; actions: AgentConfigurationAction[]; effect: AgentConfigurationEffect;
  reloadHint: string; diagnostics: AgentConfigurationDiagnostic[];
}
export interface AgentConfigurationSnapshot {
  revision: string; agentKind: AgentCliKind; environmentId: string; workspace: string | null;
  sources: AgentConfigurationSource[]; diagnostics: AgentConfigurationDiagnostic[];
}
export interface AgentConfigurationSourceRequest {
  sourceId: string; accessId: string; environmentId: string; workspace: string | null; expectedRevision: string;
}
export interface AgentConfigurationOpenRequest extends AgentConfigurationSourceRequest {
  target: AgentAssetOpenTarget; acceptedRisks: AgentAssetAccessRisk[];
}
export interface AgentConfigurationReadResult {
  sourceId: string; revision: string; text: string; truncated: boolean; diagnostics: AgentConfigurationDiagnostic[];
}
export interface AgentConfigurationEditableDocument {
  sourceId: string; label: string; path: string; format: AgentConfigurationFormat; creating: boolean;
  originalText: string; text: string;
  readOnlyReason: string | null;
}
export interface AgentResourceEditRequest { assetId: string; workspace: string | null; documentId: string | null }
export interface AgentResourceContent {
  assetId: string; category: AgentAssetCategory; description: string | null;
  facts: { label: string; value: string }[];
}
export interface AgentConfigurationEdit {
  editId: string; revision: string; expiresAt: string; agentKind: AgentCliKind;
  documents: AgentConfigurationEditableDocument[]; diagnostics: AgentConfigurationDiagnostic[];
  resource: AgentResourceContent | null;
}
export interface AgentConfigurationTextEdit { sourceId: string; text: string }
export interface AgentConfigurationSaveRequest {
  editId: string; expectedRevision: string; documents: AgentConfigurationTextEdit[];
}
export interface AgentConfigurationPlan {
  token: string; editId: string; expiresAt: string; sourceIds: string[]; changes: AgentAssetPlanChange[];
  reloadHints: string[]; diagnostics: AgentConfigurationDiagnostic[];
}
export interface AgentConfigurationApplyRequest { editId: string; planToken: string }
export interface AgentConfigurationFileResult { sourceId: string; state: AgentConfigurationFileState; message: string | null }
export interface AgentConfigurationOperation {
  id: string; editId: string; agentKind: AgentCliKind; sourceIds: string[]; revision: number;
  phase: AgentAssetOperationPhase; outcome: AgentAssetOperationOutcome | null; canCancel: boolean;
  createdAt: string; updatedAt: string; message: string | null; files: AgentConfigurationFileResult[]; reloadHints: string[];
}
export interface AgentConfigurationError { kind: AgentConfigurationErrorKind; message: string; diagnostics: AgentConfigurationDiagnostic[] }
