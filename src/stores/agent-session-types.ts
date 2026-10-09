import type {
  AgentCliKind,
  AgentRuntimeScope,
  CliSessionDetail,
  CliSessionIndexState,
  CliSessionSummary,
  TemporaryCliInstance,
  TemporaryCliPreference,
  TemporaryCliTerminalKind,
  Workspace,
} from "./provider-types";

/** Rust-owned session scope, native identity and query contracts. */
export interface AgentSessionWorkspace {
  id: string;
  path: string;
  exists: boolean;
  isHome: boolean;
}

export interface AgentSessionSource {
  id: string;
  agentKind: AgentCliKind;
  configRoot: string;
  available: boolean;
}

export interface AgentSessionScope {
  revision: string;
  workspaces: AgentSessionWorkspace[];
  sources: AgentSessionSource[];
}

export type AgentSessionRole = "main" | "subagent" | "unknown";
export type AgentSessionRoleFilter = "all" | "main" | "subagent";
export type AgentSessionParent =
  | { kind: "none" }
  | { kind: "known"; nativeId: string; parentRef: string | null }
  | { kind: "unknown" };

export interface AgentSessionRow {
  sessionRef: string;
  sourceId: string;
  workspaceId: string;
  session: CliSessionSummary;
  role: AgentSessionRole;
  parent: AgentSessionParent;
  resumeReason: string | null;
  activityState: "unknown" | "active";
  runtimeIds: string[];
}

export interface AgentSessionQuery {
  consumerId: string;
  requestId: number;
  scopeRevision: string;
  agentKinds: AgentCliKind[];
  workspaceIds: string[];
  roleFilter: AgentSessionRoleFilter;
  query: string;
  pageSize: number;
  cursor: string | null;
}

export type AgentSessionSourceStatus = "complete" | "indexing" | "partial" | "unavailable" | "unsupported" | "cancelled";

export interface AgentSessionSourceState {
  sourceId: string;
  workspaceId: string;
  state: AgentSessionSourceStatus;
  loadedCount: number;
  message: string | null;
  indexState: CliSessionIndexState;
}

export interface AgentSessionParentUpdate {
  sessionRef: string;
  parent: AgentSessionParent;
}

export interface AgentSessionCount {
  agentKind: AgentCliKind;
  loadedCount: number;
  total: number | null;
}

export interface AgentSessionCountRequest {
  consumerId: string;
  requestId: number;
  agentKinds: AgentCliKind[];
}

export interface AgentSessionCounts {
  counts: AgentSessionCount[];
  errors: Partial<Record<AgentCliKind, string>>;
}

export interface AgentSessionPage {
  snapshotId: string;
  scopeRevision: string;
  items: AgentSessionRow[];
  parentUpdates: AgentSessionParentUpdate[];
  nextCursor: string | null;
  scanPending: boolean;
  loadedCount: number;
  total: number | null;
  agentCounts: AgentSessionCount[];
  sourceStates: AgentSessionSourceState[];
}

export interface AgentSessionDetailRequest {
  consumerId: string;
  requestId: number;
  scopeRevision: string;
  sessionRef: string;
}

export interface AgentSessionDetail {
  row: AgentSessionRow;
  detail: CliSessionDetail;
}

export interface AgentSessionCancelRequest {
  consumerId: string;
  requestId: number;
}

/** Issued by Rust before terminal/Hook evidence arrives; never reconstruct in Vue. */
export interface AgentSessionLaunchIdentity {
  sessionRef: string;
  sourceIdentity: string;
  nativeSessionId: string;
  runtimeScope: AgentRuntimeScope;
}

export type AgentSessionResumeIntent =
  | { kind: "native" }
  | { kind: "provider"; providerId: string; apiKeyLocalId?: string | null };

export interface AgentSessionResumeRequest {
  requestId: string;
  sessionRef: string;
  scopeRevision: string;
  cliPath: string;
  terminalKind: TemporaryCliTerminalKind;
  intent: AgentSessionResumeIntent;
}

export type AgentSessionResumeState = "queued" | "running" | "succeeded" | "failed" | "cancelled" | "uncertain";

export interface AgentSessionResumeResult {
  instance: TemporaryCliInstance;
  workspaces: Workspace[];
  workspaceError: string | null;
  preference: TemporaryCliPreference | null;
  reused: boolean;
}

export interface AgentSessionResumeOperation {
  id: string;
  revision: number;
  requestId: string;
  sessionRef: string;
  cliKind: AgentCliKind | null;
  state: AgentSessionResumeState;
  message: string;
  createdAt: string;
  updatedAt: string;
  canCancel: boolean;
  runtimeIds: string[];
  result: AgentSessionResumeResult | null;
}
