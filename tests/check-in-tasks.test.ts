import assert from "node:assert/strict";
import test from "node:test";
import type { CheckInTask } from "../src/api/checkin.ts";
import { createCheckInTracker, type CheckInSnapshot } from "../src/utils/check-in-tasks.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function progress(revision: number, phase: CheckInTask["phase"] = "waitingHuman"): CheckInTask {
  const finished = ["completed", "failed", "cancelled", "unconfirmed"].includes(phase);
  return { runId: "run-1", providerId: "provider-1", providerName: "测试站点", batchId: null, source: "manual", revision, finished, phase,
    message: "签到验证", canResume: !finished, canCancel: !finished, startedAt: 1, finishedAt: finished ? 2 : null };
}
function api() {
  return { list: async () => [progress(1)], listen: async (_receive: (task: CheckInTask) => void) => () => {},
    submit: async (_id: string) => progress(1, "queued"), submitAll: async () => ({ batchId: "batch", tasks: [progress(1)], skipped: 0 }),
    resume: async (_id: string) => progress(2, "queued"), cancel: async (_id: string) => {} };
}

test("a delayed list cannot revive a completed check-in", async () => {
  const list = deferred<CheckInTask[]>();
  let receive!: (task: CheckInTask) => void;
  let snapshot!: CheckInSnapshot;
  const tracker = createCheckInTracker({ ...api(), list: () => list.promise,
    listen: async (callback) => { receive = callback; return () => {}; },
  }, (state) => { snapshot = state; });
  const start = tracker.start();
  receive(progress(3, "completed"));
  list.resolve([progress(1)]);
  await start;
  assert.equal(snapshot.items[0].phase, "completed");
  tracker.stop();
});

test("cancel timeout releases only that task's action state", async () => {
  const cancel = deferred<void>();
  let snapshot!: CheckInSnapshot;
  const tracker = createCheckInTracker({ ...api(), cancel: () => cancel.promise }, (state) => { snapshot = state; });
  await tracker.start();
  const request = tracker.cancel("run-1");
  assert.deepEqual(snapshot.pending, ["cancel:run-1"]);
  await tracker.submit("provider-2");
  assert.deepEqual(snapshot.pending, ["cancel:run-1"]);
  cancel.reject(new Error("取消请求超时"));
  await request;
  assert.deepEqual(snapshot.pending, []);
  assert.match(snapshot.error, /超时/);
  tracker.stop();
});

test("closing a view never waits on a pending IPC and discards its late result", async () => {
  const submit = deferred<CheckInTask>();
  let snapshot!: CheckInSnapshot;
  let updates = 0;
  const tracker = createCheckInTracker({ ...api(), submit: () => submit.promise }, (state) => { snapshot = state; updates++; });
  await tracker.start();
  const request = tracker.submit("provider-1");
  tracker.stop();
  const stoppedAt = updates;
  submit.resolve(progress(9, "completed"));
  await request;
  assert.equal(updates, stoppedAt);
  assert.equal(snapshot.items[0].phase, "waitingHuman");
});

test("a listener acquired after disposal is immediately released", async () => {
  const listener = deferred<() => void>();
  let disposed = false;
  let updates = 0;
  const tracker = createCheckInTracker({ ...api(), listen: () => listener.promise }, () => { updates++; });
  const start = tracker.start();
  tracker.stop();
  listener.resolve(() => { disposed = true; });
  await start;
  assert.equal(disposed, true);
  assert.equal(updates, 0);
});

test("waiting for human assistance is retained as a resumable task", async () => {
  let snapshot!: CheckInSnapshot;
  const resume = deferred<CheckInTask>();
  const tracker = createCheckInTracker({ ...api(), resume: () => resume.promise }, (state) => { snapshot = state; });
  await tracker.start();
  assert.equal(snapshot.items[0].finished, false);
  const request = tracker.resume("run-1");
  resume.reject(new Error("账号配置已变更"));
  await request;
  assert.equal(snapshot.items[0].canResume, true);
  assert.deepEqual(snapshot.pending, []);
  tracker.stop();
});
