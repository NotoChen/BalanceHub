import { invoke } from "./transport.ts";
export interface ModelPrice {
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
}
export type ModelPriceSource = "none" | "catalog" | "site" | "manual";
export interface ModelMetadata {
  id: string;
  name: string;
  provider: string;
  contextWindow: number | null;
  outputLimit: number | null;
  reasoning: boolean | null;
  toolCall: boolean | null;
  inputModalities: string[];
  outputModalities: string[];
  price: ModelPrice;
  /** 站点按次计费单价（USD / 次）；有值时不再按 token 估算。 */
  perRequest: number | null;
  /** 价格来源：models.dev 参考价、站点实价或卡片手动覆盖。 */
  priceSource: ModelPriceSource;
  /** 站点价格采用的分组；非站点价格时为空。 */
  priceGroup: string;
}
export interface ModelMetadataSnapshot {
  models: ModelMetadata[];
  updatedAt: string | null;
  stale: boolean;
  error: string | null;
}
/// `providerId` 让 Rust 应用该卡片的模型覆盖（名称、上下文、价格）。
export function getModelMetadata(
  ids: string[],
  refresh = false,
  providerId?: string,
) {
  return invoke<ModelMetadataSnapshot>("get_model_metadata", {
    ids,
    refresh,
    providerId: providerId ?? null,
  });
}
