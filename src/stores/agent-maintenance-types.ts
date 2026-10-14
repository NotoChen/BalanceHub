import type {
  AgentAssetOperationPhase,
  AgentAssetPlanChange,
  AgentCliKind,
} from "./provider-types";
import type {
  AgentCatalogPlan,
  AgentCatalogPlanRequest,
} from "./agent-catalog-types";

export type AgentMaintenanceKind =
  | "restore"
  | "sharedVersion"
  | "batch"
  | "skillUpdate"
  | "mcpTools"
  | "sessionRelocation";
export type AgentMaintenanceItemState =
  "pending" | "running" | "completed" | "skipped" | "failed" | "canceled";
export interface AgentHistoryEntry {
  id: string;
  title: string;
  createdAt: string;
  fileCount: number;
  bytes: number;
  paths: string[];
  error: string | null;
}
export interface AgentMaintenanceItem {
  id: string;
  label: string;
  state: AgentMaintenanceItemState;
  message: string | null;
}
export interface AgentMaintenancePlan {
  token: string;
  title: string;
  kind: AgentMaintenanceKind;
  expiresAt: string;
  changes: AgentAssetPlanChange[];
  items: AgentMaintenanceItem[];
  notes: string[];
  refresh: AgentMaintenanceRefresh;
}
export interface AgentMaintenanceRefresh {
  catalog: boolean;
  agents: AgentCliKind[];
}
export interface AgentMaintenanceOperation {
  id: string;
  planToken: string;
  kind: AgentMaintenanceKind;
  title: string;
  revision: number;
  phase: AgentAssetOperationPhase;
  outcome: "succeeded" | "partial" | "failed" | "canceled" | null;
  canCancel: boolean;
  createdAt: string;
  updatedAt: string;
  message: string;
  completed: number;
  total: number;
  items: AgentMaintenanceItem[];
  refresh: AgentMaintenanceRefresh;
}
export interface AgentSharedVersion {
  version: number;
  current: boolean;
  bytes: number;
  fileCount: number;
}
export interface AgentSkillOrigin {
  repository: string;
  reference: string;
  directory: string;
  revision: string | null;
}
export interface AgentSkillUpdateCheck {
  revision: string;
  changed: boolean;
  checkedAt: string;
  message: string;
}
export interface AgentBatchOption {
  assetId: string;
  name: string;
  request: AgentCatalogPlanRequest;
  plan: AgentCatalogPlan | null;
  reason: string | null;
}
export interface AgentMcpToolsRequest {
  assetId: string;
  bindingId: string;
  workspace: string | null;
  tools: string[];
}
export interface AgentMcpToolRule {
  name: string;
  disabledHere: boolean;
  restrictedBy: string[];
}
export interface AgentMcpToolRules {
  bindingId: string;
  rules: AgentMcpToolRule[];
  path: string;
  notes: string[];
}
