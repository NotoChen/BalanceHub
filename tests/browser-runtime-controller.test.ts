import assert from "node:assert/strict";
import test from "node:test";
import type { BrowserRuntimeStatus } from "../src/api/browser-runtime.ts";
import { createBrowserRuntimeController, type BrowserRuntimeSnapshot } from "../src/utils/browser-runtime-controller.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function status(revision: number): BrowserRuntimeStatus {
  return { phase: "notInstalled", message: "待安装", ready: false, installed: false, browser: null, systemBrowser: null,
    expectedVersion: "fixture", installedVersion: null, progress: null, revision, detectedAt: 0, canInstall: true,
    canUninstall: false, runtimeDownloadBytes: 1, browserDownloadBytes: 1 };
}
function api() {
  return { status: async (_force: boolean) => status(1), listen: async (_receive: (status: BrowserRuntimeStatus) => void) => () => {},
    install: async (_includeBrowser: boolean) => status(2), cancel: async () => {}, uninstall: async () => status(3) };
}

test("the component panel closes immediately while installation IPC is pending", async () => {
  const install = deferred<BrowserRuntimeStatus>();
  let snapshot!: BrowserRuntimeSnapshot;
  const controller = createBrowserRuntimeController({ ...api(), install: () => install.promise }, (next) => { snapshot = next; });
  await controller.start();
  controller.setVisible(true);
  const pending = controller.install(true);
  assert.equal(snapshot.pending, "install");
  controller.setVisible(false);
  assert.equal(snapshot.visible, false);
  assert.equal(snapshot.pending, "install");
  install.reject(new Error("安装请求超时"));
  await pending;
  assert.equal(snapshot.pending, "");
  assert.match(snapshot.error, /超时/);
  controller.stop();
});

test("late install acknowledgements cannot replace newer backend progress", async () => {
  const install = deferred<BrowserRuntimeStatus>();
  let receive!: (status: BrowserRuntimeStatus) => void;
  let snapshot!: BrowserRuntimeSnapshot;
  const controller = createBrowserRuntimeController({ ...api(), install: () => install.promise,
    listen: async (callback) => { receive = callback; return () => {}; },
  }, (next) => { snapshot = next; });
  await controller.start();
  const pending = controller.install(true);
  receive({ ...status(9), phase: "ready", ready: true });
  install.resolve({ ...status(2), phase: "installing" });
  await pending;
  assert.equal(snapshot.status?.phase, "ready");
  assert.equal(snapshot.pending, "");
  controller.stop();
});

test("a disposed component controller releases a late event subscription", async () => {
  const listener = deferred<() => void>();
  let disposed = false;
  const controller = createBrowserRuntimeController({ ...api(), listen: () => listener.promise }, () => {});
  const starting = controller.start();
  controller.stop();
  listener.resolve(() => { disposed = true; });
  await starting;
  assert.equal(disposed, true);
});
