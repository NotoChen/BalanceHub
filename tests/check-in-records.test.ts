import assert from "node:assert/strict";
import test from "node:test";
import type { ProviderCheckInRecord } from "../src/stores/provider-types.ts";
import {
  checkInLocalDateKey,
  checkInRecordDays,
  checkInTodayRecord,
} from "../src/utils/check-in-records.ts";

const record = (date: string, message = ""): ProviderCheckInRecord => ({
  date,
  message,
  quotaDelta: null,
});

test("check-in days count distinct dates instead of records", () => {
  const records = [
    record("2026-11-02"),
    record("2026-11-02", "补发奖励"),
    record("2026-11-03"),
  ];
  assert.equal(checkInRecordDays(records), 2);
  assert.equal(checkInRecordDays([]), 0);
});

test("today's reward only belongs to the current local month", () => {
  const now = new Date(2026, 10, 11, 9, 30);
  assert.equal(checkInLocalDateKey(now), "2026-11-11");
  const records = [record("2026-11-11"), record("2026-10-11")];

  assert.equal(checkInTodayRecord(records, "2026-11", now)?.date, "2026-11-11");
  // 查看历史月份时，本月的今天不能顶替成那个月的数据。
  assert.equal(checkInTodayRecord(records, "2026-10", now), null);
  assert.equal(checkInTodayRecord(records, "2026-12", now), null);
});
