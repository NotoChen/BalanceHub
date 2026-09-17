import assert from "node:assert/strict";
import test from "node:test";
import { effectScope, shallowRef } from "vue";
import type { CheckInBatch, CheckInTask } from "../src/api/checkin.ts";
import type { CheckInSnapshot } from "../src/utils/check-in-tasks.ts";
import { useCheckInBatchProgress } from "../src/composables/useCheckInBatchProgress.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function task(values: Partial<CheckInTask> = {}): CheckInTask {
  return {
    runId: "run-1", providerId: "provider-1", providerName: "测试中转站",
    batchId: "batch-1", source: "batch", phase: "queued", message: "等待签到",
    revision: 1, finished: false, canResume: false, canCancel: true,
    startedAt: 1000, finishedAt: null, ...values,
  };
}

function harness(submit: () => Promise<CheckInBatch | undefined>) {
  const scope = effectScope();
  const snapshot = shallowRef<CheckInSnapshot>({ items: [], pending: [], error: "" });
  const controller = scope.run(() => useCheckInBatchProgress({ snapshot: () => snapshot.value, submit }))!;
  return { scope, snapshot, ...controller };
}

test("batch progress opens before IPC finishes and remains closable without duplicate submission", async () => {
  const submission = deferred<CheckInBatch>();
  let calls = 0;
  const controller = harness(() => { calls++; return submission.promise; });
  const pending = controller.open();
  assert.equal(controller.visible.value, true);
  assert.equal(controller.progress.value.submitting, true);
  controller.visible.value = false;
  assert.equal(controller.visible.value, false);
  await controller.open();
  assert.equal(calls, 1);
  assert.equal(controller.visible.value, true);
  controller.visible.value = false;
  submission.resolve({ batchId: "batch-1", tasks: [task()], skipped: 2 });
  await pending;
  assert.equal(controller.visible.value, false);
  assert.equal(controller.progress.value.submitting, false);
  assert.equal(controller.progress.value.running, true);
  controller.scope.stop();
});

test("waiting for verification stays in progress and reopening does not enqueue again", async () => {
  let calls = 0;
  const queued = task();
  const controller = harness(async () => { calls++; return { batchId: "batch-1", tasks: [queued], skipped: 0 }; });
  await controller.open();
  controller.snapshot.value = { items: [task({ revision: 2, phase: "waitingHuman", canResume: true })], pending: [], error: "" };
  controller.visible.value = false;
  await controller.open();
  assert.equal(calls, 1);
  assert.equal(controller.progress.value.running, true);
  assert.equal(controller.progress.value.completed, false);
  assert.equal(controller.progress.value.tasks[0].phase, "waitingHuman");
  controller.snapshot.value = { items: [task({ revision: 3, phase: "completed", finished: true, finishedAt: 2500, canCancel: false })], pending: [], error: "" };
  assert.equal(controller.progress.value.running, false);
  assert.equal(controller.progress.value.completed, true);
  assert.equal(controller.progress.value.finishedAt, 2500);
  controller.scope.stop();
});

test("a reused manual task is followed by run ID and newer revisions win", async () => {
  const reused = task({ source: "manual", batchId: null, revision: 3 });
  const controller = harness(async () => ({ batchId: "new-batch", tasks: [reused], skipped: 1 }));
  await controller.open();
  controller.snapshot.value = { items: [task({ ...reused, revision: 2, phase: "checking" }), task({ runId: "unrelated" })], pending: [], error: "" };
  assert.equal(controller.progress.value.tasks.length, 1);
  assert.equal(controller.progress.value.tasks[0].revision, 3);
  controller.snapshot.value = { items: [task({ ...reused, revision: 4, phase: "unconfirmed", finished: true, finishedAt: 3000 })], pending: [], error: "" };
  assert.equal(controller.progress.value.tasks[0].phase, "unconfirmed");
  assert.equal(controller.progress.value.completed, true);
  controller.scope.stop();
});

test("frontend reload recovers the active batch with its finished rows and no new submission", async () => {
  let calls = 0;
  const controller = harness(async () => { calls++; return { batchId: "unexpected", tasks: [], skipped: 0 }; });
  controller.snapshot.value = {
    items: [
      task({ runId: "old", batchId: "old-batch", finished: true, phase: "completed", finishedAt: 500 }),
      task({ phase: "waitingBrowser", canResume: true }),
      task({ runId: "sibling", providerId: "provider-2", finished: true, phase: "completed", finishedAt: 1500 }),
      task({ runId: "manual", source: "manual", batchId: null }),
    ], pending: [], error: "",
  };
  await controller.open();
  assert.equal(calls, 0);
  assert.equal(controller.visible.value, true);
  assert.deepEqual(controller.progress.value.tasks.map((item) => item.runId), ["run-1", "sibling"]);
  assert.equal(controller.progress.value.skipped, null);
  assert.equal(controller.progress.value.running, true);
  controller.scope.stop();
});

test("a successful all-skipped batch finishes without inventing provider rows", async () => {
  const controller = harness(async () => ({ batchId: "empty", tasks: [], skipped: 4 }));
  await controller.open();
  assert.equal(controller.progress.value.completed, true);
  assert.equal(controller.progress.value.running, false);
  assert.equal(controller.progress.value.skipped, 4);
  assert.deepEqual(controller.progress.value.tasks, []);
  assert.ok(controller.progress.value.finishedAt);
  controller.scope.stop();
});

test("submission failure and tracker timeout release busy state and preserve the error", async () => {
  for (const mode of ["rejected", "tracker-timeout"] as const) {
    const pending = deferred<CheckInBatch | undefined>();
    const controller = harness(() => pending.promise);
    const request = controller.open();
    if (mode === "rejected") pending.reject(new Error("提交失败"));
    else {
      controller.snapshot.value = { items: [], pending: [], error: "提交批量签到超时" };
      pending.resolve(undefined);
    }
    await request;
    assert.equal(controller.progress.value.running, false);
    assert.equal(controller.progress.value.submitting, false);
    assert.equal(controller.progress.value.completed, false);
    assert.match(controller.progress.value.error, mode === "rejected" ? /提交失败/ : /超时/);
    controller.visible.value = false;
    assert.equal(controller.visible.value, false);
    controller.scope.stop();
  }
});

test("a disposed progress controller ignores a late submission result", async () => {
  const pending = deferred<CheckInBatch>();
  const controller = harness(() => pending.promise);
  const request = controller.open();
  controller.scope.stop();
  pending.resolve({ batchId: "late", tasks: [task()], skipped: 2 });
  await request;
  assert.equal(controller.visible.value, false);
  assert.equal(controller.progress.value.submitting, false);
  assert.deepEqual(controller.progress.value.tasks, []);
  assert.equal(controller.progress.value.completed, false);
  assert.equal(controller.progress.value.finishedAt, null);
});
