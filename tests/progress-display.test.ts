import assert from "node:assert/strict";
import test from "node:test";
import { formatProgress } from "../src/utils/progress-display.ts";

test("download and task percentages keep at most two decimals without changing their input", () => {
  const progress = 0.18 + 0.67 * (1234567 / 9876543);
  assert.equal(formatProgress(progress), "26.37%");
  assert.equal(formatProgress(0.325), "32.5%");
  assert.equal(formatProgress(0.32), "32%");
  assert.equal(formatProgress(1), "100%");
  assert.equal(formatProgress(-0.01), "0%");
  assert.equal(formatProgress(1.01), "100%");
  for (const value of [null, undefined, NaN, Infinity]) assert.equal(formatProgress(value), "—");
});
