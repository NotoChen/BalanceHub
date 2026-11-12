import { invoke } from "./transport.ts";
import type { ProviderRecommendationReport } from "../stores/provider-recommendation-types";

/**
 * 推荐由 Rust 统一计算，前端不再复制余额/测活/价格的排序规则。
 * `model` 为空时返回综合排序，不按模型过滤。
 */
export function getProviderRecommendations(model?: string) {
  return invoke<ProviderRecommendationReport>("get_provider_recommendations", {
    model: model?.trim() || null,
  });
}
