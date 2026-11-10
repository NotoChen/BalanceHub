import type { AppSettings, LivenessRecord, Provider } from "../stores/providers";

export const LIVENESS_TIMELINE_SLOTS = 24;

export function providerLivenessEnabled(provider: Provider, settings: AppSettings) {
  if (!provider.runtime.enabled || !provider.auth.apiKey.trim()) {
    return false;
  }
  return provider.liveness.useGlobal
    ? settings.livenessEnabled
    : provider.liveness.enabled;
}

/// 取最近 24 次自动测活，按时间从旧到新排列。卡片上每一格就是一次测活，
/// 不按小时合并，所以格子数、成功数和明细里的次数始终对得上。
export function livenessTimelineSlots(
  records: LivenessRecord[],
  limit = LIVENESS_TIMELINE_SLOTS,
): Array<LivenessRecord | null> {
  const recent = records
    .filter((record) => record.source === "automatic")
    .filter((record) => Number.isFinite(Number(record.checkedAt)))
    .sort((left, right) => Number(left.checkedAt) - Number(right.checkedAt))
    .slice(-limit);

  // 最右一格是最新一次；不足 24 次时左侧留空，而不是把记录挤到左边。
  const slots: Array<LivenessRecord | null> = Array.from({ length: limit }, () => null);
  const offset = limit - recent.length;
  recent.forEach((record, index) => {
    slots[offset + index] = record;
  });
  return slots;
}
