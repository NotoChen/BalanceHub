import assert from "node:assert/strict";
import test from "node:test";

import type { LivenessRecord } from "../src/stores/provider-types.ts";
import {
  LIVENESS_TIMELINE_SLOTS,
  livenessTimelineSlots,
} from "../src/utils/provider-liveness.ts";

const minute = 60 * 1000;
const base = 1_760_000_000_000;

function record(checkedAt: number, ok: boolean, source = "automatic"): LivenessRecord {
  return {
    checkedAt: String(checkedAt),
    source,
    ok,
    latencyMs: 100,
    model: "",
    baseUrl: "",
    prompt: "",
    responsePreview: "",
    message: "",
    commandPreview: "",
  };
}

test("24 次自动测活按时间顺序各占一格，最新一次在最右端", () => {
  const records = Array.from({ length: LIVENESS_TIMELINE_SLOTS }, (_, index) =>
    record(base + index * minute, index % 2 === 0),
  );

  const slots = livenessTimelineSlots([...records].reverse());

  assert.equal(slots.length, LIVENESS_TIMELINE_SLOTS);
  assert.ok(slots.every((slot) => slot !== null));
  assert.equal(slots[0]?.checkedAt, String(base));
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 1]?.checkedAt, String(base + 23 * minute));
});

test("超过 24 次时只展示最近 24 次，旧记录让出格子", () => {
  const records = Array.from({ length: 30 }, (_, index) => record(base + index * minute, true));

  const slots = livenessTimelineSlots(records);

  assert.equal(slots[0]?.checkedAt, String(base + 6 * minute));
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 1]?.checkedAt, String(base + 29 * minute));
});

test("不足 24 次时靠右排列，单次测活不占自动测活格子", () => {
  const records = [
    record(base, false),
    record(base + minute, true, "manual"),
    record(base + 2 * minute, true),
  ];

  const slots = livenessTimelineSlots(records);

  assert.equal(slots.filter((slot) => slot !== null).length, 2);
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 2]?.checkedAt, String(base));
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 1]?.checkedAt, String(base + 2 * minute));
});
