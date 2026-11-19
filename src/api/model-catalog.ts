import { invoke } from "./transport.ts";
export interface ModelPrice {
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
}
export type ModelPriceSource = "none" | "catalog" | "site" | "manual";
/** 站点计费形态；图片模型可能同时带 token 单价和按张档位。 */
export type ModelBillingMode = "token" | "perRequest" | "image" | "video";
/** 长上下文阶梯的计价基准。 */
export type ModelLongContextBasis = "wholeRequest" | "marginal";
export interface ModelPriceTier {
  /** 站点给的可读档位名，如 `≤272K`、`2K`；可能为空。 */
  label: string;
  minTokens: number;
  maxTokens: number | null;
  /** 档位内的 token 单价（USD / 百万 token）。 */
  price: ModelPrice;
  /** 按次或按张的档位单价（USD / 次、USD / 张）。 */
  perRequest: number | null;
}
export interface ModelTimePricingPeriod {
  /** 站点配置时区的 HH:MM。 */
  start: string;
  end: string;
  multiplier: number;
}
export interface ModelTimePricing {
  timezone: string;
  weekdaysOnly: boolean;
  periods: ModelTimePricingPeriod[];
}
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
  /** 站点计费形态；没有站点价格时为 token。 */
  billing: ModelBillingMode;
  /** 官方参考价（USD / 百万 token）；站点没有提供时为空。 */
  officialPrice: ModelPrice | null;
  /** 长上下文阶梯或图片分辨率档位。 */
  priceTiers: ModelPriceTier[];
  /** 多档定价的计价基准；只有长上下文阶梯会给出。 */
  longContextBasis: ModelLongContextBasis | null;
  /** 分时倍率；站点没有配置时为空。 */
  timePricing: ModelTimePricing | null;
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
