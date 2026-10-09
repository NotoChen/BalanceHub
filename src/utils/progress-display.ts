const percentFormatter = new Intl.NumberFormat("zh-CN", { style: "percent", maximumFractionDigits: 2 });

/** Progress values use 0–1; rounding belongs only to the displayed label. */
export function formatProgress(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value)
    ? percentFormatter.format(Math.max(0, Math.min(1, value)))
    : "—";
}
