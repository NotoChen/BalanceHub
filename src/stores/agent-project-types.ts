import type { AgentCliKind } from "./provider-types";
import type { SessionUsageSummary } from "./session-usage-types";
import type { UsagePeriod } from "../utils/usage-trend";

export interface AgentProjectReportRequest {
  consumerId: string;
  requestId: number;
  period: UsagePeriod;
  includeUsage?: boolean;
}

export type AgentProjectProgressPhase =
  | "scanning"
  | "decoding"
  | "finalizing";

export interface AgentProjectProgress {
  phase: AgentProjectProgressPhase;
  completed: number;
  total: number;
  message: string;
}

export interface AgentProjectAgent {
  cliKind: AgentCliKind;
  sessions: number;
  lastActivity: string | null;
}

export interface AgentProjectAccount {
  providerId: string;
  providerName: string;
  accountLabel: string;
  cliKind: AgentCliKind;
  sessions: number;
}

export interface AgentProject {
  workspaceIds: string[];
  path: string;
  sessions: number;
  undatedSessions: number;
  lastActivity: string | null;
  agents: AgentProjectAgent[];
  accounts: AgentProjectAccount[];
  usageSessions: number;
  usage: SessionUsageSummary;
}

export interface AgentProjectReport {
  period: UsagePeriod;
  generatedAt: string;
  windowStart: string;
  windowEnd: string;
  sessions: number;
  undatedSessions: number;
  decodedSessions: number;
  skippedSessions: number;
  partial: boolean;
  truncated: boolean;
  projects: AgentProject[];
  diagnostics: string[];
  accountRetentionDays: number;
  pricingError: string | null;
}
