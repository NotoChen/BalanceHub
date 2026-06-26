export type UsagePeriod = "24h" | "7d" | "30d";

export const usagePeriodOptions: { label: string; value: UsagePeriod }[] = [
  { label: "24 小时", value: "24h" },
  { label: "7 天", value: "7d" },
  { label: "30 天", value: "30d" },
];

export function usagePeriodTitle(period: UsagePeriod) {
  if (period === "24h") return "最近 24 小时用量";
  if (period === "7d") return "最近 7 天用量";
  return "最近 30 天用量";
}
