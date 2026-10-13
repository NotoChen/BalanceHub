export interface SessionTokenCounts {
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  reasoningTokens: number | null;
}
export interface SessionModelUsage {
  model: string | null;
  tokens: SessionTokenCounts;
  records: number;
  estimatedCostMicrousd: number | null;
}
export interface SessionUsageSummary {
  tokens: SessionTokenCounts;
  models: SessionModelUsage[];
  partial: boolean;
  nativeCumulative: boolean;
  records: number;
  estimatedCostMicrousd: number | null;
  unpricedRecords: number;
}
