import assert from "node:assert/strict";
import test from "node:test";
import type { ModelMetadata } from "../src/api/model-catalog.ts";
import {
  modelComparablePrice,
  modelPriceComparison,
  modelPriceDetail,
  modelPriceSourceLabel,
  modelPriceSummary,
  modelPriceTierDetail,
  modelTimePricingDetail,
  pricingScopeNote,
} from "../src/utils/model-price.ts";

function model(overrides: Partial<ModelMetadata> = {}): ModelMetadata {
  return {
    id: "gpt-5",
    name: "gpt-5",
    provider: "openai",
    contextWindow: null,
    outputLimit: null,
    reasoning: null,
    toolCall: null,
    inputModalities: [],
    outputModalities: [],
    price: { input: null, output: null, cacheRead: null, cacheWrite: null },
    perRequest: null,
    priceSource: "none",
    priceGroup: "",
    billing: "token",
    officialPrice: null,
    priceTiers: [],
    longContextBasis: null,
    timePricing: null,
    ...overrides,
  };
}

test("site price below the official reference is shown as a discount", () => {
  const subject = model({
    price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
    officialPrice: {
      input: 5,
      output: 15,
      cacheRead: null,
      cacheWrite: null,
    },
    priceSource: "site",
  });
  const comparison = modelPriceComparison(subject);
  assert.equal(comparison?.label, "5 折");
  assert.equal(comparison?.tone, "good");
  assert.match(
    comparison?.detail ?? "",
    /输入 \$2\.5 \/ 官方 \$5（5 折）；输出 \$10 \/ 官方 \$15（6\.7 折）/,
  );
  assert.equal(modelPriceComparison(null), null);
});

test("site price above the official reference is marked as a premium", () => {
  const subject = model({
    price: { input: 6, output: 12, cacheRead: null, cacheWrite: null },
    officialPrice: {
      input: 5,
      output: 15,
      cacheRead: null,
      cacheWrite: null,
    },
    priceSource: "site",
  });
  const comparison = modelPriceComparison(subject);
  assert.equal(comparison?.label, "官方 ×1.2");
  assert.equal(comparison?.tone, "warn");

  const expensive = model({
    price: { input: 10, output: 20, cacheRead: null, cacheWrite: null },
    officialPrice: {
      input: 5,
      output: 15,
      cacheRead: null,
      cacheWrite: null,
    },
    priceSource: "site",
  });
  assert.equal(modelPriceComparison(expensive)?.label, "官方 ×2");
  assert.equal(modelPriceComparison(expensive)?.tone, "bad");
});

test("identical official price is neutral and per-request models skip the comparison", () => {
  const same = model({
    price: { input: 5, output: 15, cacheRead: null, cacheWrite: null },
    officialPrice: {
      input: 5,
      output: 15,
      cacheRead: null,
      cacheWrite: null,
    },
    priceSource: "site",
  });
  assert.equal(modelPriceComparison(same)?.label, "官方同价");
  assert.equal(modelPriceComparison(same)?.tone, "neutral");

  const perRequest = model({
    perRequest: 0.04,
    billing: "perRequest",
    officialPrice: {
      input: 5,
      output: 15,
      cacheRead: null,
      cacheWrite: null,
    },
    priceSource: "site",
  });
  assert.equal(modelPriceSummary(perRequest), "$0.04/次");
  assert.equal(modelPriceComparison(perRequest), null);
});

test("image resolution tiers are summarized per image rather than per token", () => {
  const subject = model({
    billing: "image",
    priceSource: "site",
    priceTiers: [
      {
        label: "1K",
        minTokens: 0,
        maxTokens: null,
        price: { input: null, output: null, cacheRead: null, cacheWrite: null },
        perRequest: 0.04,
      },
      {
        label: "2K",
        minTokens: 0,
        maxTokens: null,
        price: { input: null, output: null, cacheRead: null, cacheWrite: null },
        perRequest: 0.08,
      },
    ],
  });
  assert.equal(modelPriceSummary(subject), "$0.04–$0.08/张");
  assert.equal(
    modelPriceTierDetail(subject),
    "分辨率档位：1K $0.04/张；2K $0.08/张",
  );
  assert.match(modelPriceDetail(subject), /分辨率档位/);
});

test("long context tiers and time pricing are described with their basis", () => {
  const subject = model({
    price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
    priceSource: "site",
    priceTiers: [
      {
        label: "≤272K",
        minTokens: 0,
        maxTokens: 272000,
        price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
        perRequest: null,
      },
      {
        label: ">272K",
        minTokens: 272000,
        maxTokens: null,
        price: { input: 5, output: 20, cacheRead: null, cacheWrite: null },
        perRequest: null,
      },
    ],
    longContextBasis: "wholeRequest",
    timePricing: {
      timezone: "Asia/Shanghai",
      weekdaysOnly: true,
      periods: [{ start: "00:30", end: "08:30", multiplier: 0.5 }],
    },
  });
  assert.equal(
    modelPriceTierDetail(subject),
    "阶梯价（整单按档，每百万 token）：≤272K $2.5 / $10；>272K $5 / $20",
  );
  assert.equal(
    modelTimePricingDetail(subject),
    "分时倍率：00:30–08:30 ×0.5 → $1.25 / $5 每百万 token（Asia/Shanghai，仅工作日）",
  );
  assert.match(modelPriceDetail(subject), /阶梯价/);
  assert.match(modelPriceDetail(subject), /分时倍率/);
});

test("image models without resolution tiers still explain their token price", () => {
  const subject = model({
    billing: "image",
    priceSource: "site",
    price: { input: 0.252, output: 2, cacheRead: null, cacheWrite: null },
  });
  assert.equal(modelPriceSummary(subject), "$0.252 / $2");
  assert.match(modelPriceDetail(subject), /图片计费模型/);
});

test("missing prices produce no summary instead of a fake zero", () => {
  const subject = model({ priceSource: "catalog" });
  assert.equal(modelPriceSummary(subject), "");
  assert.equal(modelPriceDetail(subject), "");
});

test("cross-site comparison only uses the site price in the displayed unit", () => {
  const token = model({
    price: { input: 2.5, output: 10, cacheRead: null, cacheWrite: null },
    priceSource: "site",
    officialPrice: { input: 5, output: 15, cacheRead: null, cacheWrite: null },
  });
  assert.equal(modelPriceSourceLabel(token), "站点价格");
  assert.equal(modelComparablePrice(token), 2.5);

  const grouped = model({
    price: { input: 2, output: null, cacheRead: null, cacheWrite: null },
    priceSource: "site",
    priceGroup: "vip",
  });
  assert.equal(modelPriceSourceLabel(grouped), "站点价格（分组 vip）");
  assert.equal(modelComparablePrice(grouped), 2);

  const catalog = model({
    price: { input: 1, output: 2, cacheRead: null, cacheWrite: null },
    priceSource: "catalog",
  });
  assert.equal(modelPriceSourceLabel(catalog), "公开参考价");
  assert.equal(modelComparablePrice(catalog), null);

  const image = model({
    billing: "image",
    priceSource: "site",
    perRequest: 0.08,
    price: { input: 0.25, output: 2, cacheRead: null, cacheWrite: null },
  });
  assert.equal(modelComparablePrice(image), 0.08);

  const perRequest = model({
    billing: "perRequest",
    priceSource: "manual",
    perRequest: 0.04,
  });
  assert.equal(modelPriceSourceLabel(perRequest), "卡片覆盖价格");
  assert.equal(modelComparablePrice(perRequest), 0.04);
});

test("token comparison requires an input price from the same entry tier", () => {
  const outputOnly = model({
    price: { input: null, output: 40, cacheRead: null, cacheWrite: null },
    priceSource: "site",
  });
  // 只有输出单价时无法和其它站的输入单价同口径比较；价格仍展示，但不排序。
  assert.equal(modelPriceSummary(outputOnly), "$40");
  assert.equal(modelComparablePrice(outputOnly), null);

  const tierOnly = model({
    priceSource: "site",
    priceTiers: [
      {
        label: "≤128K",
        minTokens: 0,
        maxTokens: 128000,
        price: { input: 3, output: 12, cacheRead: null, cacheWrite: null },
        perRequest: null,
      },
      {
        label: ">128K",
        minTokens: 128000,
        maxTokens: null,
        price: { input: 6, output: 24, cacheRead: null, cacheWrite: null },
        perRequest: null,
      },
    ],
  });
  // 基础价为空时，展示和比价都用第一档；不能拿后一档的价格来排序。
  assert.equal(modelPriceSummary(tierOnly), "$3 / $12");
  assert.equal(modelComparablePrice(tierOnly), 3);
});

test("per-request models show tier prices when the base request price is absent", () => {
  const tiered = model({
    billing: "perRequest",
    priceSource: "site",
    priceTiers: [
      {
        label: "标准",
        minTokens: 0,
        maxTokens: null,
        price: { input: null, output: null, cacheRead: null, cacheWrite: null },
        perRequest: 0.02,
      },
      {
        label: "长文本",
        minTokens: 0,
        maxTokens: null,
        price: { input: null, output: null, cacheRead: null, cacheWrite: null },
        perRequest: 0.04,
      },
    ],
  });
  // 站点只给档位按次价时，列表不能显示为空，也不能编造 token 单价。
  assert.equal(modelPriceSummary(tiered), "$0.02–$0.04/次");
  assert.equal(modelComparablePrice(tiered), 0.02);

  const video = model({ billing: "video", priceSource: "site", perRequest: 0.5 });
  assert.equal(modelPriceSummary(video), "$0.5/次");
  assert.equal(modelComparablePrice(video), 0.5);
});

test("pricing scope note explains exclusive and group rates without duplicating prices", () => {
  assert.equal(
    pricingScopeNote({
      rateMultiplier: 1.2,
      userRateMultiplier: 0.8,
      peakRateEnabled: true,
      peakStart: "09:00",
      peakEnd: "12:00",
      peakRateMultiplier: 1.5,
      longContextPricingEnabled: false,
    }),    "价格已按专属倍率 ×0.8 计算（分组 ×1.2） · 高峰 09:00–12:00 ×1.5，当前为高峰外价格 · 分组未启用长上下文阶梯，价格按基础档展示",
  );
  assert.equal(
    pricingScopeNote({
      rateMultiplier: 1,
      userRateMultiplier: null,
      peakRateEnabled: false,
      peakStart: "",
      peakEnd: "",
      peakRateMultiplier: null,
      longContextPricingEnabled: null,
    }),
    "",
  );
  assert.equal(pricingScopeNote(null), "");
});
