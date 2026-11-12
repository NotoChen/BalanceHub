import type { AgentCliKind } from "./provider-types";
import type { SessionUsageSummary } from "./session-usage-types";

export interface AgentUsageReportRequest {
  consumerId: string;
  requestId: number;
  period: "24h" | "7d" | "30d";
  includeSiteUsage?: boolean;
}

export interface AgentUsageAccount {
  providerId: string | null;
  /** 空字符串表示未关联到任何 BalanceHub 中转站。 */
  providerName: string;
  /** 启动时记录的账号展示快照；空表示未记录。 */
  accountLabel: string;
  cliKind: AgentCliKind;
  sessions: number;
  usage: SessionUsageSummary;
}

export interface AgentUsageProject {
  path: string;
  sessions: number;
  usage: SessionUsageSummary;
}

export interface AgentUsageSiteSummary {
  requestCount: number;
  tokenUsed: number;
}

export interface AgentUsageSite {
  providerId: string;
  providerName: string;
  /** 本地统计中归属该中转站的 token 总量。 */
  localTokens: number;
  error: string | null;
  usage: AgentUsageSiteSummary | null;
}

export interface AgentUsageReport {
  period: "24h" | "7d" | "30d";
  generatedAt: string;
  windowStart: string;
  windowEnd: string;
  scannedSessions: number;
  countedSessions: number;
  skippedSessions: number;
  undatedSessions: number;
  partial: boolean;
  truncated: boolean;
  totals: SessionUsageSummary;
  accounts: AgentUsageAccount[];
  projects: AgentUsageProject[];
  sites: AgentUsageSite[];
  diagnostics: string[];
  accountRetentionDays: number;
  pricingError: string | null;
}
