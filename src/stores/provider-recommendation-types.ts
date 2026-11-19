/// 后端 `get_provider_recommendations` 的结果结构；评分只在同一份报告内比较。
export interface ProviderRecommendation {
  providerId: string;
  label: string;
  baseUrl: string;
  score: number;
  eligible: boolean;
  model: string;
  modelSupported: boolean | null;
  modelWireName: string;
  inputPrice: number | null;
  outputPrice: number | null;
  perRequestPrice: number | null;
  livenessSuccessRate: number | null;
  livenessSamples: number;
  /** 站点近 24 小时观测到的该模型成功率（0..1）。 */
  siteSuccessRate: number | null;
  siteAvgTps: number | null;
  siteAvgLatencyMs: number | null;
  quotaRemainingPercent: number | null;
  quotaLabel: string;
  resetsAt: number | null;
  quotaUpdatedAt: string | null;
  pricingUpdatedAt: string | null;
  livenessUpdatedAt: string | null;
  livenessStale: boolean;
  quotaStale: boolean;
  pricingStale: boolean;
  cooldownUntil: number | null;
  cooldownReason: string | null;
  reasons: string[];
  blockers: string[];
}

export interface ProviderRecommendationReport {
  model: string;
  generatedAt: string;
  recommendedProviderId: string | null;
  entries: ProviderRecommendation[];
}
