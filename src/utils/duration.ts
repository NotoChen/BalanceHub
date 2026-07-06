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

export function secondsToDurationValue(seconds: number, unit: DurationUnit) {
  return Number((seconds / durationUnitSeconds(unit)).toFixed(2));
}

export function durationValueToSeconds(value: number | undefined, unit: DurationUnit) {
  return Math.max(0, Math.round(Number(value || 0) * durationUnitSeconds(unit)));
}
