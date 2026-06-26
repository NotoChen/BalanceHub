import { computed } from "vue";
import type { ProviderUsageSummary } from "../stores/providers";
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
    usageChartPoints,
    usageLinePath,
    usagePeakPoint,
    usageRequestTotal,
    usageTokenTotal,
    usageTotal,
    usageXAxisLabels,
    usageYAxisLabels,
  };
}
