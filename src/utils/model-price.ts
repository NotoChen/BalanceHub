import type {
  ModelMetadata,
  ModelPrice,
  ModelPriceTier,
} from "../api/model-catalog";
import type { ProviderGroupPricing } from "../stores/provider-types";

const tokenCountFormat = new Intl.NumberFormat("zh-CN");

/** 金额按量级保留有效位，避免 $0.04 被显示成 $0.0400 或 $0.0000。 */
export function formatModelPrice(value: number): string {
  if (!Number.isFinite(value)) return "—";
  const digits = value >= 1 ? 2 : value >= 0.01 ? 4 : 6;
  return `$${+value.toFixed(digits)}`;
}

function tokenAmount(price: ModelPrice, multiplier = 1): string {
  const amounts = [price.input, price.output]
    .filter((value): value is number => value != null)
    .map((value) => formatModelPrice(value * multiplier));
  return amounts.join(" / ");
}

/// 站点只返回阶梯价时基础价可能为空；展示和比价都按第一档（入门档）取值。
function entryTokenPrice(model: ModelMetadata): ModelPrice {
  return model.price.input == null && model.price.output == null && model.priceTiers.length
    ? model.priceTiers[0].price
    : model.price;
}

/** 表格主价格：token 单价、按次价，或图片按张价。 */
export function modelPriceSummary(model: ModelMetadata | null | undefined): string {
  if (!model) return "";
  if (model.billing !== "token") {
    // 按次、按张或视频计费：站点可能只给分辨率/时长档位价，没有基础单价；
    // 档位价优先，缺失时才退回单个按次价或图片 token 单价。
    const unit = model.billing === "image" ? "/张" : "/次";
    const tierPrices = model.priceTiers
      .map((tier) => tier.perRequest)
      .filter((value): value is number => value != null);
    if (tierPrices.length) {
      const min = Math.min(...tierPrices);
      const max = Math.max(...tierPrices);
      return min === max
        ? `${formatModelPrice(min)}${unit}`
        : `${formatModelPrice(min)}–${formatModelPrice(max)}${unit}`;
    }
    if (model.perRequest != null) {
      return `${formatModelPrice(model.perRequest)}${unit}`;
    }
    return tokenAmount(model.price);
  }
  if (model.perRequest != null) {
    return `${formatModelPrice(model.perRequest)}/次`;
  }
  return tokenAmount(entryTokenPrice(model));
}

/** 价格来源；跨站比价时用它区分站点实价和公开参考价。 */
export function modelPriceSourceLabel(model: ModelMetadata | null | undefined): string {
  if (!model) return "";
  if (model.priceSource === "site") {
    return model.priceGroup ? `站点价格（分组 ${model.priceGroup}）` : "站点价格";
  }
  if (model.priceSource === "manual") return "卡片覆盖价格";
  if (model.priceSource === "catalog") return "公开参考价";
  return "";
}

/**
 * 用于跨站排序的单一价格；只取站点实价或卡片覆盖价。
 * token 只比输入单价：各站必须同口径，缺输入价时按未知处理，不能拿输出价
 * 去和别的站的输入价排在一起；按次按一次价、图片按一张价。
 */
export function modelComparablePrice(
  model: ModelMetadata | null | undefined,
): number | null {
  if (!model || model.priceSource === "none" || model.priceSource === "catalog") {
    return null;
  }
  const finite = (value: number | null | undefined) =>
    value != null && Number.isFinite(value) ? value : null;
  if (model.billing === "token") {
    return finite(entryTokenPrice(model).input);
  }
  const direct = finite(model.perRequest);
  if (direct != null) return direct;
  const tierPrices = model.priceTiers
    .map((item) => finite(item.perRequest))
    .filter((value): value is number => value != null)
    .sort((left, right) => left - right);
  return tierPrices[0] ?? null;
}

function formatRate(value: number): string {
  return String(+value.toFixed(3));
}

/** 价格已经按分组/专属倍率换算；这里只解释可能让账单高于列表价的因素。 */
export function pricingScopeNote(
  group: ProviderGroupPricing | null | undefined,
): string {
  if (!group) return "";
  const parts: string[] = [];
  const userRate = group.userRateMultiplier;
  const rate = group.rateMultiplier;
  if (userRate != null && userRate !== rate) {
    parts.push(
      `价格已按专属倍率 ×${formatRate(userRate)} 计算${rate != null ? `（分组 ×${formatRate(rate)}）` : ""}`,
    );
  } else if (rate != null && rate !== 1) {
    parts.push(`价格已按分组倍率 ×${formatRate(rate)} 计算`);
  }
  if (
    group.peakRateEnabled &&
    group.peakStart &&
    group.peakEnd &&
    group.peakRateMultiplier != null
  ) {
    parts.push(
      `高峰 ${group.peakStart}–${group.peakEnd} ×${formatRate(group.peakRateMultiplier)}，当前为高峰外价格`,
    );
  }
  if (group.longContextPricingEnabled === false) {
    parts.push("分组未启用长上下文阶梯，价格按基础档展示");
  }
  return parts.join(" · ");
}

export interface ModelPriceComparison {
  /** 站点价 / 官方参考价；1 表示同价。 */
  ratio: number;
  /** 可直接展示的折扣或溢价标签。 */
  label: string;
  tone: "good" | "warn" | "bad" | "neutral";
  /** 悬停用的逐项对比。 */
  detail: string;
}

function discountLabel(ratio: number): string {
  if (Math.abs(ratio - 1) < 0.005) return "官方同价";
  if (ratio < 1) {
    const discount = ratio * 10;
    return `${+discount.toFixed(discount < 1 ? 2 : 1)} 折`;
  }
  return `官方 ×${+ratio.toFixed(2)}`;
}

function comparisonTone(ratio: number): ModelPriceComparison["tone"] {
  if (ratio < 0.95) return "good";
  if (ratio <= 1.05) return "neutral";
  if (ratio <= 1.5) return "warn";
  return "bad";
}

/** 站点实价与官方参考价的对比；按 token 计费的模型才有意义。 */
export function modelPriceComparison(
  model: ModelMetadata | null | undefined,
): ModelPriceComparison | null {
  if (!model || model.billing !== "token" || !model.officialPrice) return null;
  const inputRatio =
    model.price.input != null && model.officialPrice.input
      ? model.price.input / model.officialPrice.input
      : null;
  const outputRatio =
    model.price.output != null && model.officialPrice.output
      ? model.price.output / model.officialPrice.output
      : null;
  const ratio = inputRatio ?? outputRatio;
  if (ratio == null || !Number.isFinite(ratio) || ratio < 0) return null;
  // 徽标同时承担标签和颜色两种信息，必须用同一个比率。此前标签取输入、
  // 颜色取两者最差值，输入便宜而输出贵时会出现“5 折”配红色告警的
  // 自相矛盾；逐项差异仍完整保留在悬停详情里。

  const details: string[] = [];
  if (inputRatio != null) {
    details.push(
      `输入 ${formatModelPrice(model.price.input!)} / 官方 ${formatModelPrice(
        model.officialPrice.input!,
      )}（${discountLabel(inputRatio)}）`,
    );
  }
  if (outputRatio != null) {
    details.push(
      `输出 ${formatModelPrice(model.price.output!)} / 官方 ${formatModelPrice(
        model.officialPrice.output!,
      )}（${discountLabel(outputRatio)}）`,
    );
  }
  return {
    ratio,
    label: discountLabel(ratio),
    tone: comparisonTone(ratio),
    detail: details.join("；"),
  };
}

function tierRange(tier: ModelPriceTier): string {
  if (tier.label) return tier.label;
  const min = tokenCountFormat.format(tier.minTokens);
  return tier.maxTokens != null
    ? `${min}–${tokenCountFormat.format(tier.maxTokens)} token`
    : `>${min} token`;
}

function tierAmount(model: ModelMetadata, tier: ModelPriceTier): string {
  if (tier.perRequest != null) {
    return `${formatModelPrice(tier.perRequest)}/${model.billing === "image" ? "张" : "次"}`;
  }
  return tokenAmount(tier.price);
}

/** 长上下文阶梯或图片分辨率档位的逐档说明。 */
export function modelPriceTierDetail(
  model: ModelMetadata | undefined,
): string {
  if (!model?.priceTiers.length) return "";
  const tiers = model.priceTiers
    .map((tier) => `${tierRange(tier)} ${tierAmount(model, tier)}`)
    .join("；");
  if (model.billing === "image") {
    return `分辨率档位：${tiers}`;
  }
  const basis =
    model.longContextBasis === "wholeRequest"
      ? "整单按档"
      : model.longContextBasis === "marginal"
        ? "仅超出部分按新档"
        : "分档计价";
  return `阶梯价（${basis}，每百万 token）：${tiers}`;
}

/** 时段内的折后单价；图片档位价不适合用单一金额概括，只说明倍率。 */
function discountedAmount(model: ModelMetadata, multiplier: number): string {
  if (model.billing === "image") return "";
  if (model.perRequest != null) {
    return `${formatModelPrice(model.perRequest * multiplier)}/次`;
  }
  const amounts = tokenAmount(entryTokenPrice(model), multiplier);
  return amounts ? `${amounts} 每百万 token` : "";
}

export function modelTimePricingDetail(
  model: ModelMetadata | undefined,
): string {
  const time = model?.timePricing;
  if (!model || !time?.periods.length) return "";
  const periods = time.periods
    .map((period) => {
      const label = `${period.start}–${period.end} ×${+period.multiplier.toFixed(2)}`;
      const amount = discountedAmount(model, period.multiplier);
      return amount ? `${label} → ${amount}` : label;
    })
    .join("；");
  const scope = [
    time.timezone,
    time.weekdaysOnly ? "仅工作日" : "",
  ].filter(Boolean);
  return `分时倍率：${periods}${scope.length ? `（${scope.join("，")}）` : ""}`;
}

/** 价格悬停说明：官方对比、阶梯、分时和图片计费口径。 */
export function modelPriceDetail(model: ModelMetadata | null | undefined): string {
  if (!model) return "";
  const details = [
    modelPriceComparison(model)?.detail ?? "",
    modelPriceTierDetail(model),
    modelTimePricingDetail(model),
  ].filter(Boolean);
  if (model.billing === "image" && !model.priceTiers.length) {
    details.push("图片计费模型；单价以站点账单为准");
  }
  return details.join("；");
}
