import assert from "node:assert/strict";
import test from "node:test";
import { quotaLevel } from "../src/utils/quota-level.ts";

test("one threshold set decides when a quota figure warns or turns critical", () => {
  assert.equal(quotaLevel(null), "unknown");
  assert.equal(quotaLevel(Number.NaN), "unknown");
  assert.equal(quotaLevel(0), "ok");
  assert.equal(quotaLevel(69.9), "ok");
  assert.equal(quotaLevel(70), "warn");
  assert.equal(quotaLevel(89.9), "warn");
  assert.equal(quotaLevel(90), "high");
  assert.equal(quotaLevel(100), "high");
});
