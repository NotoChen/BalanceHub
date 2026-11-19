import type { ModelBillingMode, ModelMetadata } from "../api/model-catalog";
import type { AgentCliKind, Provider, ProviderModelPerformance } from "../stores/provider-types";
import { oauthAgentCliKind } from "./oauth-cli.ts";
import { providerDisplayLabel, providerQuotaHeadline } from "./provider-display.ts";
import {
  type ModelPriceComparison,
  modelComparablePrice,
  modelPriceComparison,
  modelPriceDetail,
  modelPriceSourceLabel,
  modelPriceSummary,
  pricingScopeNote,
} from "./model-price.ts";

/** 展示名对应的上游模型名；没有覆盖配置时就是模型名本身。 */
export function providerModelWireName(
  provider: Provider,
  model: string,
): string {
  const name = model.trim();
  const wire = provider.modelConfig?.overrides?.[name]?.wireName?.trim();
  return wire || name;
}

function hasOwn(record: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key);
}

/**
 * 卡片是否能拿到这个模型。模型列表已同步时以展示规则后的列表为准，避免把
 * 已隐藏的模型重新拉进比价；列表为空（尚未同步）时才退回覆盖项和站点价目。
 */
export function providerOffersModel(provider: Provider, model: string): boolean {
  const name = model.trim();
  if (!name) return false;
  const wire = providerModelWireName(provider, name);
  const visible = provider.visibleModels ?? [];
  if (visible.length) {
    return (
      visible.includes(name) ||
      visible.includes(wire) ||
      visible.some((id) => providerModelWireName(provider, id) === name)
    );
  }
  const overrides = provider.modelConfig?.overrides ?? {};
  if (hasOwn(overrides, name)) return true;
  const pricing = provider.capabilities?.pricing?.models ?? {};
  return hasOwn(pricing, name) || (wire !== name && hasOwn(pricing, wire));
}

/** 提供该模型的卡片，按 id 去重；同一张卡只出现一次。 */
export function modelComparisonProviders(
  providers: readonly Provider[],
  model: string,
): Provider[] {
  const seen = new Set<string>();
  return providers.filter((provider) => {
    if (!providerOffersModel(provider, model)) return false;
    const id = provider.identity.id;
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  });
}

export function providerModelPerformance(
  provider: Provider,
  model: string,
): ProviderModelPerformance | undefined {
  const metrics = provider.capabilities?.performance?.models;
  if (!metrics) return undefined;
  const name = model.trim();
  const wire = providerModelWireName(provider, name);
  return metrics[name] ?? (wire !== name ? metrics[wire] : undefined);
}

export function formatSuccessRate(value: number): string {
  return `${value.toFixed(value >= 99.9 ? 2 : 1)}%`;
}

export function formatThroughput(value: number): string {
  return `${+value.toFixed(1)} t/s`;
}

export function formatLatency(value: number): string {
  return value >= 1000 ? `${+(value / 1000).toFixed(2)}s` : `${Math.round(value)}ms`;
}

function performanceFacts(provider: Provider, model: string): string {
  const metric = providerModelPerformance(provider, model);
  if (!metric) return "";
  const facts: string[] = [];
  if (metric.successRate != null && Number.isFinite(metric.successRate)) {
    facts.push(`可用性 ${formatSuccessRate(metric.successRate)}`);
  }
  if (metric.avgTps != null && Number.isFinite(metric.avgTps)) {
    facts.push(formatThroughput(metric.avgTps));
  }
  if (metric.avgLatencyMs != null && Number.isFinite(metric.avgLatencyMs)) {
    facts.push(`延迟 ${formatLatency(metric.avgLatencyMs)}`);
  }
  return facts.join(" · ");
}

export interface ModelComparisonInput {
  provider: Provider;
  metadata: ModelMetadata | null;
  error?: string;
}

export interface ModelComparisonRow {
  providerId: string;
  label: string;
  current: boolean;
  disabled: boolean;
  metadata: ModelMetadata | null;
  error: string;
  price: string;
  priceTitle: string;
  comparison: ModelPriceComparison | null;
  detail: string;
  sourceLabel: string;
  group: string;
  scopeNote: string;
  billing: ModelBillingMode | null;
  sortPrice: number | null;
  lowest: boolean;
  quota: string;
  performance: string;
  /// 这张卡片能不能把自己的凭据写进 Agent 默认配置。
  canSwitch: boolean;
  /// 这张卡片能不能直接启动临时 CLI；与写默认配置是两种独立能力。
  canLaunch: boolean;
  /// OAuth 授权只接受本家 Agent；中转站和网关留空表示不限定。
  allowedKind: AgentCliKind | null;
}

function offerRank(row: ModelComparisonRow): number {
  if (row.sortPrice != null) return 0;
  if (row.metadata) return 1;
  return 2;
}

/**
 * 跨站比价行：站点实价/覆盖价按单位价格排序，公开参考价和缺失资料排在后面；
 * 只有单位一致且至少两行有价格时才标记最低价，避免拿 token 价和按张价硬比。
 */
export function buildModelComparisonRows(
  model: string,
  inputs: readonly ModelComparisonInput[],
  currentProviderId?: string,
): ModelComparisonRow[] {
  const rows = inputs.map(({ provider, metadata, error }) => {
    const quota = providerQuotaHeadline(provider);
    const summary = modelPriceSummary(metadata);
    const source = modelPriceSourceLabel(metadata);
    return {
      providerId: provider.identity.id,
      label:
        providerDisplayLabel(provider) ||
        provider.identity.id ||
        "未命名中转站",
      current: provider.identity.id === currentProviderId,
      disabled: provider.runtime?.enabled === false,
      metadata,
      error: error?.trim() ?? "",
      price: summary,
      priceTitle: [summary, source, modelPriceDetail(metadata)]
        .filter(Boolean)
        .join(" · "),
      comparison: modelPriceComparison(metadata),
      detail: modelPriceDetail(metadata),
      sourceLabel: source || "暂无价格资料",
      group: metadata?.priceGroup?.trim() ?? "",
      scopeNote: pricingScopeNote(provider.capabilities?.pricing?.groupPricing),
      billing: metadata?.billing ?? null,
      sortPrice: modelComparablePrice(metadata),
      lowest: false,
      quota: [quota.label, quota.value].filter(Boolean).join(" "),
      performance: performanceFacts(provider, model),
      canSwitch: provider.actions?.defaultCliConfig === true,
      canLaunch: provider.actions?.temporaryCli === true,
      allowedKind: provider.auth?.oauth
        ? oauthAgentCliKind(provider.auth.oauth.vendor)
        : null,
    };
  });

  rows.sort((left, right) => {
    const rank = offerRank(left) - offerRank(right);
    if (rank) return rank;
    if (left.sortPrice != null && right.sortPrice != null) {
      const amount = left.sortPrice - right.sortPrice;
      if (amount) return amount;
    }
    return left.label.localeCompare(right.label);
  });

  const priced = rows.filter(
    (row): row is ModelComparisonRow & { sortPrice: number } =>
      row.sortPrice != null && !row.disabled,
  );
  const billings = new Set(priced.map((row) => row.billing));
  if (priced.length > 1 && billings.size === 1) {
    const minimum = Math.min(...priced.map((row) => row.sortPrice));
    for (const row of priced) {
      row.lowest = row.sortPrice === minimum;
    }
  }
  return rows;
}
