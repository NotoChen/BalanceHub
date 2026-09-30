// IPC mirrors of models/agent_lifecycle.rs. Rust owns channel and action eligibility.
import type {
  AgentAssetOperationOutcome,
  AgentAssetOperationPhase,
  AgentCliKind,
  AgentInstallation,
} from "./provider-types";

export type AgentLifecycleActionKind = "upgrade";
export type AgentLifecycleChannel = "npm" | "homebrewFormula" | "homebrewCask" | "vendorNative" | "unverified";
export type AgentLifecycleVersionSource = "homebrew" | "npmRegistry" | "vendorRelease" | "unknown";
export type AgentLifecycleVersionState =
  | "notChecked"
  | "unsupported"
  | "unknown"
  | "checkFailed"
  | "upToDate"
  | "updateAvailable"
  | "aheadOfLatest";
export type AgentLifecycleUnavailableReason =
  | "runtimeUnavailable"
  | "unsupportedChannel"
  | "unsupportedPlatform"
  | "installationUnavailable"
  | "provenanceUnverified"
  | "directoryConflict"
  | "permissionRequired"
  | "versionUnavailable"
  | "alreadyCurrent";

export interface AgentLifecycleAction {
  kind: AgentLifecycleActionKind;
  available: boolean;
  reason: AgentLifecycleUnavailableReason | null;
  reasonMessage: string | null;
}

export interface AgentLifecycleVersion {
  state: AgentLifecycleVersionState;
  source: AgentLifecycleVersionSource;
  latestVersion: string | null;
  checkedAt: string | null;
  lastSuccessAt: string | null;
  nextCheckAt: string | null;
  stale: boolean;
  message: string | null;
}

export interface AgentLifecycleTarget {
  id: string;
  agentKind: AgentCliKind;
  label: string;
  installation: AgentInstallation;
  isCurrent: boolean;
  channel: AgentLifecycleChannel;
  channelLabel: string;
  releaseTrack: string | null;
  directory: string | null;
  evidenceRevision: string;
  version: AgentLifecycleVersion;
  actions: AgentLifecycleAction[];
}

export type AgentLifecycleVersionRefresh = "cached" | "ifStale" | "force";

export interface AgentLifecycleCatalogRequest {
  agentKind?: AgentCliKind | null;
  versionRefresh?: AgentLifecycleVersionRefresh;
}

export interface AgentLifecycleCatalog {
  targets: AgentLifecycleTarget[];
  refreshedAt: string;
  nextCheckAt: string | null;
}

export interface AgentLifecyclePlanRequest {
  agentKind: AgentCliKind;
  targetId: string;
  action: AgentLifecycleActionKind;
  expectedEvidenceRevision: string;
}

export interface AgentLifecyclePlan {
  planToken: string;
  agentKind: AgentCliKind;
  targetId: string;
  installationId: string;
  action: AgentLifecycleActionKind;
  channel: AgentLifecycleChannel;
  channelLabel: string;
  directory: string;
  fromVersion: string | null;
  /** Release observed during planning; the updater selects the installed version. */
  toVersion: string;
  mechanismId: string;
  changes: string[];
  commandPreview: string[];
  affectedInstallationIds: string[];
  confirmationMessage: string;
  cancellationBoundary: string;
  timeoutSeconds: number;
  expiresAt: string;
}

export interface AgentLifecycleApplyRequest {
  planToken: string;
  agentKind: AgentCliKind;
  targetId: string;
  action: AgentLifecycleActionKind;
}

export interface AgentLifecycleOperation {
  id: string;
  agentKind: AgentCliKind;
  targetId: string;
  installationId: string;
  action: AgentLifecycleActionKind;
  channel: AgentLifecycleChannel;
  channelLabel: string;
  directory: string;
  fromVersion: string | null;
  /** Release observed during planning; the updater selects the installed version. */
  toVersion: string;
  observedVersion: string | null;
  recovered: boolean;
  nextLaunch: { executablePath: string | null; version: string | null; usesUpgradedInstallation: boolean; message: string } | null;
  verifiedExecutablePath: string | null;
  phase: AgentAssetOperationPhase;
  canCancel: boolean;
  revision: number;
  createdAt: string;
  updatedAt: string;
  outcome: AgentAssetOperationOutcome | null;
  message: string | null;
  timedOut: boolean;
  outputTruncated: boolean;
  commandPreview: string[];
  diagnostics: { stdout: string; stderr: string; exitCode: number | null; error: string | null; truncated: boolean } | null;
}

export type AgentLifecycleErrorKind =
  | "invalidRequest"
  | "planExpired"
  | "planConsumed"
  | "actorMismatch"
  | "targetMismatch"
  | "actionMismatch"
  | "targetChanged"
  | "actionUnavailable"
  | "versionCheckFailed"
  | "operationNotFound"
  | "capacityExceeded"
  | "internalFailure";

export interface AgentLifecycleError {
  kind: AgentLifecycleErrorKind;
  message: string;
  reason: AgentLifecycleUnavailableReason | null;
}
