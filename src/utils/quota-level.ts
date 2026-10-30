export type QuotaLevel = "ok" | "warn" | "high" | "unknown";
/// One threshold set for every quota meter in the app, so an OAuth window and a
/// relay balance never disagree about when a figure turns amber or red. The
/// input is the percentage already used, which is how providers report quota.
export function quotaLevel(percent: number | null | undefined): QuotaLevel {
  if (percent == null || !Number.isFinite(percent)) return "unknown";
  if (percent >= 90) return "high";
  if (percent >= 70) return "warn";
  return "ok";
}
