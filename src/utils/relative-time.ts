/// Cards answer "how fresh is this" in words; an absolute clock reading forces
/// the reader to subtract timestamps on every glance.
export function elapsedLabel(value: number | null | undefined, now = Date.now()): string {
  if (value == null || !Number.isFinite(value) || value <= 0) return "";
  const seconds = Math.max(0, Math.floor((now - value) / 1000));
  if (seconds < 60) return "刚刚";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} 天前`;
  return `${Math.floor(days / 30)} 个月前`;
}
