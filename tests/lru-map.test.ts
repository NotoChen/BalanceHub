import assert from "node:assert/strict";
import test from "node:test";
import { pruneLruEntries, setLruEntry, touchLruEntry } from "../src/utils/lru-map.ts";

test("LRU cache evicts the least recently used entry", () => {
  const cache = new Map<string, number>();
  setLruEntry(cache, "a", 1, 2);
  setLruEntry(cache, "b", 2, 2);
  assert.equal(touchLruEntry(cache, "a"), 1);

  setLruEntry(cache, "c", 3, 2);

  assert.deepEqual([...cache.keys()], ["a", "c"]);
});

test("LRU cache pruning removes entries outside the active scope", () => {
  const cache = new Map([
    ["provider-a", 1],
    ["provider-b", 2],
  ]);

  pruneLruEntries(cache, (key) => key === "provider-b");

  assert.deepEqual([...cache.entries()], [["provider-b", 2]]);
});
