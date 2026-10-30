/// Sizes CPA reports for its log files and the terminal index reports for its
/// own store. Rounded to one decimal so a few MiB does not print as a long
/// fraction; anything not a positive finite number reads as empty.
export function formatFileSize(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 / 1024).toFixed(1)} MiB`;
}
