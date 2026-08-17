import assert from "node:assert/strict";
import test from "node:test";

import type { Provider } from "../src/stores/provider-types.ts";
import {
  mergeProvidersByRevision,
  pruneProviderTombstones,
} from "../src/utils/provider-revision.ts";

function provider(id: string, revision: number, name = id) {
  return {
    revision,
    identity: { id, name },
  } as unknown as Provider;
}

test("late provider updates cannot resurrect a removed card", () => {
  const merged = mergeProvidersByRevision([], [provider("removed", 2)], 1, { removed: 3 });
  assert.deepEqual(merged, []);
});

test("a genuinely newer provider can replace a removal tombstone", () => {
  const recreated = provider("removed", 4, "recreated");
  const merged = mergeProvidersByRevision([], [recreated], 1, { removed: 3 });
  assert.deepEqual(merged, [recreated]);
});

test("a full snapshot blocks absent providers from older incremental responses", () => {
  const stale = provider("stale", 4);
  const current = provider("current", 5);
  assert.deepEqual(mergeProvidersByRevision([current], [stale], 5, {}), [current]);
});

test("newer current cards are not overwritten and covered tombstones are pruned", () => {
  const current = provider("provider", 6, "current");
  const stale = provider("provider", 5, "stale");
  assert.deepEqual(mergeProvidersByRevision([current], [stale], 4, {}), [current]);
  assert.deepEqual(pruneProviderTombstones({ old: 3, future: 7 }, 5), { future: 7 });
});
