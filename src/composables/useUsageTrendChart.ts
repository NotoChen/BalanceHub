import { computed } from "vue";
import type { ProviderUsageModelStat, ProviderUsageSummary } from "../stores/providers";
import { usagePeriodTitle, type UsagePeriod } from "../utils/usage-trend.ts";

interface UseUsageTrendChartOptions {
  summary: ProviderUsageSummary | null;
  period: UsagePeriod;
}

export function useUsageTrendChart(options: UseUsageTrendChartOptions) {
  const title = computed(() => usagePeriodTitle(options.period));

  const maxUsageValue = computed(() => {
    const points = options.summary?.points ?? [];
    return Math.max(0, ...points.map((point) => point.used)) || 1;
  });

  const usageTotal = computed(() =>
    (options.summary?.points ?? []).reduce((total, point) => total + point.used, 0),
  );

  const usageRequestTotal = computed(() =>
    (options.summary?.points ?? []).reduce((total, point) => total + point.requestCount, 0),
  );

  const usageTokenTotal = computed(() =>
    (options.summary?.points ?? []).reduce((total, point) => total + point.tokenUsed, 0),
  );

  /// 24 小时视图只有 Sub2API 能给到小时桶，NewAPI 的用量接口按整天归档；
  /// 写死“按小时汇总”会让后者的日桶柱子被当成小时数据读。
  const usagePeriodGranularity = computed(() => {
    if (options.period !== "24h") return "按天汇总";
    const points = options.summary?.points ?? [];
    return points.some((point) => /^\d{2}-\d{2} \d{2}:00$/.test(point.date)) ? "按小时汇总" : "按天汇总";
  });

  const usageTimeRangeMinutes = computed(() => {
    if (options.period === "24h") return 24 * 60;
    if (options.period === "7d") return 7 * 24 * 60;
    return 30 * 24 * 60;
  });

  const usageAverageRpm = computed(() => safeDivide(usageRequestTotal.value, usageTimeRangeMinutes.value));

  const usageAverageTpm = computed(() => safeDivide(usageTokenTotal.value, usageTimeRangeMinutes.value));

  const usageModelStats = computed(() =>
    [...(options.summary?.modelStats ?? [])]
      .filter((item) => item.requestCount > 0 || item.used > 0 || item.tokenUsed > 0)
      .sort(sortModelStats),
  );

  const usageTopQuotaModels = computed(() =>
    usageModelStats.value
      .filter((item) => item.used > 0)
      .slice(0, 8),
  );

  const usageTopCallModels = computed(() =>
    [...usageModelStats.value]
      .sort((left, right) => right.requestCount - left.requestCount || right.used - left.used)
      .filter((item) => item.requestCount > 0)
      .slice(0, 8),
  );

  /// 站点自带的性能采样：成功率和耗时说的是“这条路稳不稳”，和消耗额度无关。
  /// 按成功率从低到高排，最该避开的路由排在最前。
  const usageModelHealth = computed(() => {
    const requests = new Map(
      (options.summary?.modelStats ?? []).map((item) => [item.modelName, item.requestCount]),
    );
    return (options.summary?.modelHealth ?? [])
      .filter(
        (item) =>
          item.successRate !== null ||
          item.avgLatencyMs !== null ||
          item.avgTps !== null,
      )
      .map((item) => ({
        ...item,
        requestCount: requests.get(item.modelName) ?? 0,
      }))
      .sort(
        (left, right) =>
          (left.successRate ?? 101) - (right.successRate ?? 101) ||
          (right.avgLatencyMs ?? 0) - (left.avgLatencyMs ?? 0),
      )
      .slice(0, 8);
  });

  const usageMaxModelQuota = computed(() => Math.max(0, ...usageTopQuotaModels.value.map((item) => item.used)) || 1);

  const usageMaxModelCalls = computed(() => Math.max(1, ...usageTopCallModels.value.map((item) => item.requestCount)));

  const usageChartPoints = computed(() => {
    const points = options.summary?.points ?? [];
    if (points.length === 0) return [];
    const max = maxUsageValue.value;
    const width = 560;
    const height = 220;
    const paddingLeft = 54;
    const paddingRight = 18;
    const paddingTop = 26;
    const paddingBottom = 38;
    const plotBottom = height - paddingBottom;
    const step = points.length === 1 ? 0 : (width - paddingLeft - paddingRight) / (points.length - 1);
    return points.map((point, index) => {
      const x = paddingLeft + step * index;
      const y = plotBottom - (point.used / max) * (height - paddingTop - paddingBottom);
      return { ...point, x, y };
    });
  });

  const usageLinePath = computed(() =>
    usageChartPoints.value
      .map((point, index) => `${index === 0 ? "M" : "L"} ${point.x.toFixed(1)} ${point.y.toFixed(1)}`)
      .join(" "),
  );

  const usageAreaPath = computed(() => {
    const points = usageChartPoints.value;
    if (points.length === 0) return "";
    const first = points[0];
    const last = points[points.length - 1];
    return `${usageLinePath.value} L ${last.x.toFixed(1)} 182 L ${first.x.toFixed(1)} 182 Z`;
  });

  const usageYAxisLabels = computed(() => {
    if (!options.summary) return [];
    const max = maxUsageValue.value;
    return [
      { value: max, y: 26 },
      { value: max / 2, y: 104 },
      { value: 0, y: 182 },
    ];
  });

  const usageXAxisLabels = computed(() => {
    const points = usageChartPoints.value;
    if (points.length === 0) return [];
    const indexes = Array.from(new Set([0, Math.floor((points.length - 1) / 2), points.length - 1]));
    return indexes.map((index) => points[index]).filter(Boolean);
  });

  const usagePeakPoint = computed(() => {
    const points = usageChartPoints.value;
    if (points.length === 0) return null;
    const peak = points.reduce((current, point) => (point.used > current.used ? point : current), points[0]);
    return peak.used > 0 ? peak : null;
  });

  return {
    maxUsageValue,
    title,
    usageAreaPath,
    usageAverageRpm,
    usageAverageTpm,
    usageChartPoints,
    usageLinePath,
    usageMaxModelCalls,
    usageMaxModelQuota,
    usageModelHealth,
    usagePeakPoint,
    usagePeriodGranularity,
    usageRequestTotal,
    usageTokenTotal,
    usageTopCallModels,
    usageTopQuotaModels,
    usageTotal,
    usageXAxisLabels,
    usageYAxisLabels,
  };
}

function safeDivide(value: number, divisor: number) {
  const result = value / divisor;
  if (!Number.isFinite(result)) return 0;
  return result;
}

function sortModelStats(left: ProviderUsageModelStat, right: ProviderUsageModelStat) {
  return right.used - left.used || right.requestCount - left.requestCount || right.tokenUsed - left.tokenUsed;
}
