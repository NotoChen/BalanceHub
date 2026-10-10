import assert from "node:assert/strict";
import test from "node:test";
import {
  createCloudSyncController,
  type CloudSyncView,
} from "../src/utils/cloud-sync-controller.ts";
import type {
  CloudSyncSnapshot,
  CloudSyncComparison,
} from "../src/api/cloud-sync.ts";
import { withTimeout } from "../src/utils/promise-timeout.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function state(revision: number): CloudSyncSnapshot {
  return {
    settings: {
      serverUrl: "http://fixture.invalid/",
      username: "fixture",
      remoteRoot: "BalanceHub",
      deviceName: "测试设备",
      autoSync: false,
      hasPassword: true,
      hasPassphrase: true,
    },
    hasRecovery: false,
    status: {
      revision,
      taskId: "cloud-sync-fixture",
      phase: "review",
      message: "首次同步",
      running: false,
      canCancel: true,
      progress: null,
      startedAt: 1,
      finishedAt: null,
      lastSyncedAt: null,
      retryAt: null,
      uploaded: 0,
      downloaded: 0,
      transferredBytes: 0,
      automatic: false,
      review: {
        id: "review-fixture",
        initial: true,
        remoteDevice: "另一台测试设备",
        remoteUpdatedAt: 1,
        changes: [
          {
            key: "setting/themeMode",
            title: "主题",
            category: "偏好",
            conflict: true,
            upload: true,
            download: true,
            localDeleted: false,
            remoteDeleted: false,
          },
        ],
      },
    },
  };
}
function api() {
  return {
    state: async () => state(1),
    save: async () => state(2),
    sync: async () => state(2),
    test: async () => state(2),
    cancel: async () => state(2),
    restore: async () => state(2),
    confirm: async () => state(2),
    compare: async (): Promise<CloudSyncComparison> => ({
      title: "主题",
      files: [],
    }),
    listen: async (_receive: (value: CloudSyncSnapshot) => void) => () => {},
  };
}

test("sync confirmation closes immediately while IPC is pending and remains independent of the main view", async () => {
  const request = deferred<CloudSyncSnapshot>();
  let view!: CloudSyncView;
  const controller = createCloudSyncController(
    { ...api(), confirm: () => request.promise },
    (next) => {
      view = next;
    },
  );
  await controller.start();
  assert.equal(view.reviewVisible, true);
  const waiting = controller.confirm([
    { key: "setting/themeMode", side: "remote" },
  ]);
  assert.equal(view.reviewVisible, false);
  assert.equal(view.pending, "confirm");
  controller.setReviewVisible(false);
  request.resolve({
    ...state(3),
    status: {
      ...state(3).status,
      running: true,
      phase: "uploading",
      review: null,
    },
  });
  assert.equal(await waiting, true);
  assert.equal(view.pending, "");
  assert.equal(view.snapshot?.status.running, true);
  assert.equal(view.reviewVisible, false);
  controller.stop();
});

test("failed and timed out requests always release controls and preserve the review", async () => {
  let view!: CloudSyncView;
  const controller = createCloudSyncController(
    {
      ...api(),
      confirm: () =>
        withTimeout(
          new Promise<CloudSyncSnapshot>(() => {}),
          5,
          "fixture timeout",
        ),
    },
    (next) => {
      view = next;
    },
  );
  await controller.start();
  assert.equal(
    await controller.confirm([{ key: "setting/themeMode", side: "local" }]),
    false,
  );
  assert.equal(view.pending, "");
  assert.equal(view.reviewVisible, true);
  assert.match(view.error, /timeout/);
  controller.stop();
});

test("late IPC responses cannot overwrite newer progress or resurrect an old review", async () => {
  const request = deferred<CloudSyncSnapshot>();
  let receive!: (snapshot: CloudSyncSnapshot) => void;
  let view!: CloudSyncView;
  const controller = createCloudSyncController(
    {
      ...api(),
      sync: () => request.promise,
      listen: async (callback) => {
        receive = callback;
        return () => {};
      },
    },
    (next) => {
      view = next;
    },
  );
  await controller.start();
  const waiting = controller.sync();
  receive({
    ...state(9),
    status: {
      ...state(9).status,
      phase: "failed",
      message: "fixture upload failed",
      review: null,
      canCancel: false,
      finishedAt: 9,
    },
  });
  request.resolve(state(2));
  await waiting;
  assert.equal(view.snapshot?.status.phase, "failed");
  assert.equal(view.snapshot?.status.revision, 9);
  assert.equal(view.reviewVisible, false);
  controller.stop();
});

test("closing or changing the selected diff discards stale content without locking the window", async () => {
  const first = deferred<CloudSyncComparison>();
  let view!: CloudSyncView;
  const controller = createCloudSyncController(
    {
      ...api(),
      compare: (_id, key) =>
        key === "first"
          ? first.promise
          : Promise.resolve({ title: "second", files: [] }),
    },
    (next) => {
      view = next;
    },
  );
  await controller.start();
  const pending = controller.compare("first");
  await controller.compare("second");
  first.resolve({ title: "old", files: [] });
  await pending;
  assert.equal(view.comparison?.title, "second");
  controller.setReviewVisible(false);
  assert.equal(view.comparison, null);
  assert.equal(view.comparing, false);
  controller.stop();
});

test("unmount releases a delayed event listener and ignores a late diff", async () => {
  const listener = deferred<() => void>();
  let removed = false;
  let updates = 0;
  const controller = createCloudSyncController(
    { ...api(), listen: () => listener.promise },
    () => {
      updates++;
    },
  );
  const starting = controller.start();
  controller.stop();
  listener.resolve(() => {
    removed = true;
  });
  await starting;
  assert.equal(removed, true);
  assert.equal(updates, 0);
});
