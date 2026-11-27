import assert from "node:assert/strict";
import test from "node:test";
import { formatDuration, formatDurationMs } from "../src/utils/duration.ts";

test("毫秒耗时在一秒内显示毫秒，超过后显示秒", () => {
  assert.equal(formatDurationMs(0), "0 ms");
  assert.equal(formatDurationMs(999), "999 ms");
  assert.equal(formatDurationMs(1000), "1.0 s");
  assert.equal(formatDurationMs(12_340), "12.3 s");
  // 系统时钟回拨会算出负耗时：按 0 展示，不把负数渲染到进度窗口。
  assert.equal(formatDurationMs(-50), "0 ms");
});

test("秒级耗时保留小时、分钟和秒的自然单位", () => {
  assert.equal(formatDuration(0), "0 秒");
  assert.equal(formatDuration(3660), "1 小时 1 分钟");
  assert.equal(formatDuration(-1), "未设置");
});
