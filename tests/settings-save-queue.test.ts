import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createSettingsSaveQueue, type SettingsSaveState } from "../src/utils/settings-save-queue.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
async function settle() { for (let index = 0; index < 16; index++) await Promise.resolve(); }

function harness(t: TestContext, write: (value: { count: number }) => Promise<{ count: number }>, timeoutMs = 1000) {
  let draft = { count: 0 };
  let state: SettingsSaveState = "saved";
  let error = "";
  const failures: string[] = [];
  const queue = createSettingsSaveQueue({ read: () => ({ ...draft }), write, accept: (value) => { draft = value; },
    state: (value, message) => { state = value; error = message; }, failed: (message) => failures.push(message), timeoutMs });
  t.after(queue.dispose);
  return { queue, draft: () => draft, edit: (count: number) => { draft.count = count; queue.schedule(); }, state: () => state, error: () => error, failures };
}

test("a slow save cannot overwrite newer edits when the settings window closes", async (t) => {
  const first = deferred<{ count: number }>();
  const second = deferred<{ count: number }>();
  const writes: number[] = [];
  const context = harness(t, (value) => { writes.push(value.count); return writes.length === 1 ? first.promise : second.promise; });
  context.edit(1);
  const saving = context.queue.flush();
  context.edit(2);
  assert.equal(context.queue.acceptExternal({ count: 0 }), false);
  first.resolve({ count: 1 });
  await settle();
  assert.equal(context.draft().count, 2);
  assert.deepEqual(writes, [1, 2]);
  second.resolve({ count: 2 });
  assert.equal(await saving, true);
  assert.equal(context.state(), "saved");
  assert.equal(context.queue.hasPendingChanges(), false);
});

test("failed settings retain their draft and can be explicitly retried", async (t) => {
  let attempts = 0;
  const context = harness(t, async (value) => { if (++attempts === 1) throw new Error("磁盘不可写"); return value; });
  context.edit(3);
  assert.equal(await context.queue.flush(), false);
  assert.equal(context.state(), "error");
  assert.equal(context.draft().count, 3);
  assert.equal(context.queue.acceptExternal({ count: 0 }), false);
  assert.equal(await context.queue.flush(), true);
  assert.equal(context.state(), "saved");
  assert.equal(context.error(), "");
  assert.equal(attempts, 2);
});

test("a timeout releases the UI but keeps native writes serialized until the old IPC settles", async (t) => {
  const native = deferred<{ count: number }>();
  const writes: number[] = [];
  const context = harness(t, async (value) => { writes.push(value.count); return writes.length === 1 ? native.promise : value; }, 5);
  context.edit(1);
  assert.equal(await context.queue.flush(), false);
  assert.equal(context.state(), "error");
  context.edit(2);
  assert.equal(await context.queue.flush(), false);
  assert.deepEqual(writes, [1], "retry must not race the unacknowledged native write");
  native.resolve({ count: 1 });
  await settle();
  assert.deepEqual(writes, [1, 2]);
  assert.equal(context.draft().count, 2);
  assert.equal(context.state(), "saved");
});

test("reverting a draft during an older save persists the reversion after that save", async (t) => {
  const old = deferred<{ count: number }>();
  const writes: number[] = [];
  const context = harness(t, async (value) => { writes.push(value.count); return writes.length === 1 ? old.promise : value; });
  context.edit(1);
  const saving = context.queue.flush();
  context.edit(0);
  old.resolve({ count: 1 });
  await saving;
  assert.deepEqual(writes, [1, 0]);
  assert.equal(context.draft().count, 0);
  assert.equal(context.state(), "saved");
});

test("backend normalization and object key order do not cause an autosave loop", async (t) => {
  let draft = { interval: 30, mode: "fixed" };
  let calls = 0;
  let saved = false;
  const queue = createSettingsSaveQueue({ read: () => ({ ...draft }), write: async () => { calls++; return { mode: "fixed", interval: 30 }; },
    accept: (value) => { draft = value; }, state: (state) => { saved = state === "saved"; }, failed: (message) => assert.fail(message) });
  t.after(queue.dispose);
  draft.interval = 0;
  await queue.flush();
  assert.equal(calls, 1);
  assert.equal(draft.interval, 30);
  assert.equal(saved, true);
  assert.equal(queue.hasPendingChanges(), false);
});

test("restoring configuration holds later saves and rebases only edits made while waiting", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let draft = { theme: "light", interval: 30, notification: { name: "before", enabled: true } };
  const native = deferred<{ settings: typeof draft; result: string }>();
  const writes: typeof draft[] = [];
  let state: SettingsSaveState = "saved";
  const queue = createSettingsSaveQueue({
    read: () => structuredClone(draft),
    write: async (value) => { writes.push(value); return value; },
    accept: (value) => { draft = value; },
    state: (value) => { state = value; },
    failed: (message) => assert.fail(message),
  });
  t.after(queue.dispose);
  const restoring = queue.replace(() => native.promise, 5);
  assert.equal(queue.acceptExternal({ ...draft, interval: 0 }), false);
  t.mock.timers.tick(5);
  assert.equal(state, "error", "a delayed restore must not leave indefinite saving feedback");
  draft.theme = "dark";
  draft.notification.name = "edited while waiting";
  queue.schedule();
  assert.equal(await queue.flush(), false);
  assert.deepEqual(writes, [], "the old configuration cannot be written over an in-flight restore");
  native.resolve({ settings: { theme: "system", interval: 120, notification: { name: "restored", enabled: false } }, result: "restored" });
  assert.equal(await restoring, "restored");
  await settle();
  assert.deepEqual(draft, { theme: "dark", interval: 120, notification: { name: "edited while waiting", enabled: false } });
  assert.deepEqual(writes, [draft]);
  assert.equal(state, "saved");
});

test("a failed restore releases its place in the queue without losing later edits", async (t) => {
  const context = harness(t, async (value) => value);
  let reject!: (error: Error) => void;
  const restoring = context.queue.replace(() => new Promise<{ settings: { count: number }; result: void }>((_resolve, fail) => { reject = fail; }));
  await settle();
  context.edit(9);
  reject(new Error("备份格式错误"));
  await assert.rejects(restoring, /备份格式错误/);
  await settle();
  assert.equal(context.draft().count, 9);
  assert.equal(context.state(), "saved");
  assert.equal(context.queue.hasPendingChanges(), false);
});
