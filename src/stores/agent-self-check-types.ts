import type { AgentCliKind } from "./provider-types";

export type AgentSelfCheckStatus = "ready" | "degraded" | "unavailable" | "unknown";
export type AgentSelfCheckArea = "executable" | "configuration" | "sessionHistory" | "hook";
export type AgentSelfCheckSeverity = "info" | "warning" | "error";

export interface AgentSelfCheckIssue {
  area: AgentSelfCheckArea;
  severity: AgentSelfCheckSeverity;
  message: string;
  detail: string;
}

export interface AgentSelfCheckHook {
  state: "not_installed" | "installed_untrusted" | "installed_unverified" | "healthy"
    | "disabled" | "conflict" | "helper_missing" | "spool_blocked" | "unsupported";
  installed: boolean;
  enabled: boolean;
  configPath: string;
  detail: string;
}

export interface AgentSelfCheckEntry {
  agentKind: AgentCliKind;
  label: string;
  status: AgentSelfCheckStatus;
  executable: string;
  version: string;
  configurationSources: number;
  sessionSourcesTotal: number;
  sessionSourcesAvailable: number;
  sessionHistory: boolean;
  hookInventory: boolean;
  hook: AgentSelfCheckHook | null;
  issues: AgentSelfCheckIssue[];
}

export interface AgentSelfCheckReport {
  checkedAt: string;
  status: AgentSelfCheckStatus;
  ready: number;
  degraded: number;
  unavailable: number;
  unknown: number;
  entries: AgentSelfCheckEntry[];
}
