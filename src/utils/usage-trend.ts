import type { ProviderUsageOverview } from "../stores/provider-types";

export type UsagePeriod = "24h" | "7d" | "30d";

export const usagePeriodOptions: { label: string; value: UsagePeriod }[] = [
  { label: "今日", value: "24h" },
  { label: "7 天", value: "7d" },
  { label: "30 天", value: "30d" },
];

export function usagePeriodTitle(period: UsagePeriod) {
  if (period === "24h") return "今日用量";
  if (period === "7d") return "最近 7 天用量";
  return "最近 30 天用量";
}

/**
 * 平均 RPM/TPM 的分母：优先使用后端返回的实际统计窗口（今日是从本地零点
 * 到现在），缺失时才按周期名估算。
 */
export function usagePeriodMinutes(period: UsagePeriod, windowMinutes?: number) {
  if (Number.isFinite(windowMinutes) && (windowMinutes ?? 0) > 0) {
    return windowMinutes!;
  }
  if (period === "24h") {
    const now = new Date();
    const start = new Date(now);
    start.setHours(0, 0, 0, 0);
    return Math.max(1, Math.floor((now.getTime() - start.getTime()) / 60_000));
  }
  return period === "7d" ? 7 * 24 * 60 : 30 * 24 * 60;
}

/**
 * 全站汇总的读数：模型排行只保留前几项，涉及模型数必须取后端的去重计数，
 * 不能拿排行列表长度冒充总数。平均 RPM 与单站面板共用同一套窗口口径。
 */
export function usageOverviewStats(
  overview: ProviderUsageOverview | null | undefined,
  period: UsagePeriod,
) {
  const modelStats = overview?.modelStats ?? [];
  const minutes = usagePeriodMinutes(period, overview?.windowMinutes);
  return {
    requests: overview?.requestCount ?? 0,
    models: overview?.modelCount ?? 0,
    rpm: (overview?.requestCount ?? 0) / minutes,
    modelStats,
    /// 排行条以汇总列表里最大的次数为满格，和单站的模型排行同一口径。
    maxCalls: Math.max(1, ...modelStats.map((item) => item.requestCount)),
    modelHealth: overview?.modelHealth ?? [],
    errors: overview?.errors ?? [],
  };
}
