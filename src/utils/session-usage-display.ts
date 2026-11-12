import type { SessionUsageSummary } from "../stores/session-usage-types";

export function usageCount(value: number | null | undefined) {
  return value == null ? "未知" : value.toLocaleString("zh-CN");
}

export function usageTokenTotal(tokens: SessionUsageSummary["tokens"]) {
  return (tokens.inputTokens ?? 0) + (tokens.outputTokens ?? 0);
}

export function usageTokenKnown(tokens: SessionUsageSummary["tokens"]) {
  return tokens.inputTokens != null || tokens.outputTokens != null;
}

export function usageTokenAmount(tokens: SessionUsageSummary["tokens"]) {
  return usageTokenKnown(tokens) ? usageCount(usageTokenTotal(tokens)) : "未知";
}

export function usageTokenLabel(tokens: SessionUsageSummary["tokens"]) {
  return usageTokenKnown(tokens)
    ? `${usageCount(usageTokenTotal(tokens))} token`
    : "未知 token";
}

export function usageCostValue(
  micros: number | null | undefined,
  records: number,
) {
  if (micros == null) return records ? "价格未知" : "无用量";
  const value = micros / 1_000_000;
  return `$${value.toLocaleString("en-US", { maximumFractionDigits: 4 })}`;
}

export function usageCostLabel(usage: SessionUsageSummary) {
  return usageCostValue(usage.estimatedCostMicrousd, usage.records);
}

export function usagePriceNote(usage: SessionUsageSummary) {
  if (!usage.unpricedRecords) return "";
  return `${usageCount(usage.unpricedRecords)} 条用量缺少价格资料`;
}
