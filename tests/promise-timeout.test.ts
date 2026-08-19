import assert from "node:assert/strict";
import test from "node:test";

import { withTimeout } from "../src/utils/promise-timeout.ts";

test("bounded async operations return their result before the deadline", async () => {
  assert.equal(await withTimeout(Promise.resolve("ok"), 100, "timeout"), "ok");
});

test("bounded async operations release callers when the operation never settles", async () => {
  await assert.rejects(
    withTimeout(new Promise<never>(() => {}), 5, "operation timed out"),
    /operation timed out/,
  );
});
