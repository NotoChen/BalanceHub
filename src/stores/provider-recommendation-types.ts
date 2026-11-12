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
  livenessSuccessRate: number | null;
  livenessSamples: number;
  quotaRemainingPercent: number | null;
  quotaLabel: string;
  resetsAt: number | null;
  reasons: string[];
  blockers: string[];
}

export interface ProviderRecommendationReport {
  model: string;
  generatedAt: string;
  recommendedProviderId: string | null;
  entries: ProviderRecommendation[];
}
