import type { ProviderCheckInRecord } from "../stores/provider-types";

/// 签到日历按本地自然日归档；月份和日期必须是同一套本地时区，
/// 否则跨时区时“今天”会落到相邻的格子里。
export function checkInLocalDateKey(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

/// 站点同一天可能返回多条记录（例如补发奖励），「签到天数」按日期去重，
/// 不能拿记录条数当天数。
export function checkInRecordDays(records: readonly ProviderCheckInRecord[]) {
  return new Set(records.map((record) => record.date)).size;
}

/// 「今日奖励」只属于当前自然月：翻到历史月份时不能拿本月今天的数据顶替。
export function checkInTodayRecord(
  records: readonly ProviderCheckInRecord[],
  month: string,
  now = new Date(),
): ProviderCheckInRecord | null {
  const today = checkInLocalDateKey(now);
  if (month !== today.slice(0, 7)) return null;
  return records.find((record) => record.date === today) ?? null;
}
