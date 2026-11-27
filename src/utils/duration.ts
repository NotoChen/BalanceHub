export type DurationUnit = "second" | "minute" | "hour";

export const durationUnitOptions: { label: string; value: DurationUnit }[] = [
  { label: "秒", value: "second" },
  { label: "分钟", value: "minute" },
  { label: "小时", value: "hour" },
];

export function durationUnitSeconds(unit: DurationUnit) {
  if (unit === "hour") return 3600;
  if (unit === "minute") return 60;
  return 1;
}

export function durationValueToSeconds(value: number | undefined, unit: DurationUnit) {
  return Math.max(0, Math.round(Number(value || 0) * durationUnitSeconds(unit)));
}

export function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) return "未设置";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainingSeconds = seconds % 60;
  return [
    hours ? `${hours} 小时` : "",
    minutes ? `${minutes} 分钟` : "",
    remainingSeconds ? `${remainingSeconds} 秒` : "",
  ].filter(Boolean).join(" ") || "0 秒";
}

/// 毫秒级耗时（探测步骤、批量任务）：1 秒以内显示毫秒，超过后保留一位小数。
export function formatDurationMs(milliseconds: number) {
  const value = Math.max(0, milliseconds);
  return value < 1000 ? `${value} ms` : `${(value / 1000).toFixed(1)} s`;
}
