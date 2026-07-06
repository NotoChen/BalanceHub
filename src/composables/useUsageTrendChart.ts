import { computed } from "vue";
import type { ProviderUsageModelStat, ProviderUsageSummary } from "../stores/providers";
import { usagePeriodTitle, type UsagePeriod } from "../utils/usage-trend";

interface UseUsageTrendChartOptions {
  summary: ProviderUsageSummary | null;
  period: UsagePeriod;
}

export function useUsageTrendChart(options: UseUsageTrendChartOptions) {
  const title = computed(() => usagePeriodTitle(options.period));

  const maxUsageValue = computed(() => {
    const points = options.summary?.points ?? [];
    return Math.max(1, ...points.map((point) => point.used));
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

  const usageAverage = computed(() => {
    const count = options.summary?.points.length ?? 0;
    return count > 0 ? usageTotal.value / count : 0;
  });

  const usageTimeRangeMinutes = computed(() => {
    if (options.period === "24h") return 24 * 60;
    if (options.period === "7d") return 7 * 24 * 60;
    return 30 * 24 * 60;
  });

  const usageAverageRpm = computed(() => safeDivide(usageRequestTotal.value, usageTimeRangeMinutes.value, 3));

  const usageAverageTpm = computed(() => safeDivide(usageTokenTotal.value, usageTimeRangeMinutes.value, 3));

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

  const usageMaxModelQuota = computed(() => Math.max(1, ...usageTopQuotaModels.value.map((item) => item.used)));

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
    return points.reduce((peak, point) => (point.used > peak.used ? point : peak), points[0]);
  });

  return {
    maxUsageValue,
    title,
    usageAreaPath,
    usageAverage,
    usageAverageRpm,
    usageAverageTpm,
    usageChartPoints,
    usageLinePath,
    usageMaxModelCalls,
    usageMaxModelQuota,
    usageModelStats,
    usagePeakPoint,
    usageRequestTotal,
    usageTimeRangeMinutes,
    usageTokenTotal,
    usageTopCallModels,
    usageTopQuotaModels,
    usageTotal,
    usageXAxisLabels,
    usageYAxisLabels,
  };
}

function safeDivide(value: number, divisor: number, precision = 3) {
  const result = value / divisor;
  if (!Number.isFinite(result)) return 0;
  const factor = 10 ** precision;
  return Math.round(result * factor) / factor;
}

function sortModelStats(left: ProviderUsageModelStat, right: ProviderUsageModelStat) {
  return right.used - left.used || right.requestCount - left.requestCount || right.tokenUsed - left.tokenUsed;
}
