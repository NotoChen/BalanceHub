import assert from "node:assert/strict";
import test from "node:test";
import { loadSource } from "./helpers/load-source.ts";

type Callback = () => unknown;

function ref<T>(value: T) {
  return { value };
}

function setup(
  startupError: string | null,
  overrides: Record<string, unknown> = {},
) {
  // onMounted 会同步平台样式类，并登记可见性监听；Node 测试环境没有 DOM。
  const documentListeners = new Map<string, Callback[]>();
  const fakeDocument = {
    visibilityState: "visible" as "visible" | "hidden",
    documentElement: {
      classList: { add: () => {}, remove: () => {} },
    },
    addEventListener: (type: string, callback: Callback) => {
      documentListeners.set(type, [...(documentListeners.get(type) ?? []), callback]);
    },
    removeEventListener: (type: string, callback: Callback) => {
      documentListeners.set(
        type,
        (documentListeners.get(type) ?? []).filter((item) => item !== callback),
      );
    },
  };
  (globalThis as { document?: unknown }).document = fakeDocument;
  const windowListeners = new Map<string, Callback[]>();
  (globalThis as { window?: unknown }).window = {
    addEventListener: (type: string, callback: Callback) => {
      windowListeners.set(type, [...(windowListeners.get(type) ?? []), callback]);
    },
    removeEventListener: (type: string, callback: Callback) => {
      windowListeners.set(
        type,
        (windowListeners.get(type) ?? []).filter((item) => item !== callback),
      );
    },
  };
  const mounted: Callback[] = [];
  const unmounted: Callback[] = [];
  const loadErrorWatchers: Callback[] = [];
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  let launchSyncs = 0;
  let probes = 0;
  const loadError = ref<string | null>(startupError);
  const { useAppLifecycle } = loadSource<{
    useAppLifecycle: (options: Record<string, unknown>) => void;
  }>("composables/useAppLifecycle.ts", {
    vue: {
      onMounted: (callback: Callback) => mounted.push(callback),
      onUnmounted: (callback: Callback) => unmounted.push(callback),
      watch: (source: unknown, callback: Callback) => {
        if (source === loadError) loadErrorWatchers.push(callback);
      },
    },
    "./useHostPlatform": {
      useHostPlatform: () => ({ ensure: async () => "macos" }),
    },
    "../api/transport.ts": {
      listen: async (
        event: string,
        handler: (event: { payload: unknown }) => void,
      ) => {
        listeners.set(event, handler);
        return () => listeners.delete(event);
      },
    },
  });

  useAppLifecycle({
    loadError,
    settings: ref({ themeMode: "auto" }),
    settingsForm: { themeMode: "auto" },
    settingsDrawerVisible: ref(false),
    initialize: async () => {},
    syncFromSettings: () => {},
    setupThemeListener: () => {},
    cleanupThemeListener: () => {},
    syncLaunchAtLogin: async () => {
      launchSyncs += 1;
    },
    autoProbeCliTools: async () => {
      probes += 1;
    },
    reloadProviders: () => {},
    onStorageBlocked: () => {},
    applyTheme: () => {},
    flushSettingsSave: () => {},
    resetProviderPointerDrag: () => {},
    ...overrides,
  });

  return {
    mounted,
    unmounted,
    loadError,
    loadErrorWatchers,
    listeners,
    doc: fakeDocument,
    documentListeners,
    windowListeners,
    launchSyncs: () => launchSyncs,
    probes: () => probes,
  };
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

/// 定向补拉在同一帧内合并，测试要等这一帧结束再断言。
function providerFlushDone() {
  return new Promise((resolve) => setTimeout(resolve, 30));
}

test("startup work deferred by a load error runs once after the config is recovered", async () => {
  const lifecycle = setup("解析配置失败");
  await lifecycle.mounted[0]();

  // 启动读取失败时不探测 CLI、不同步开机启动，避免拿默认配置写回系统状态。
  assert.equal(lifecycle.launchSyncs(), 0);
  assert.equal(lifecycle.probes(), 0);

  // 用户修好 config.json 并点击「重新读取」后，loadError 清空。
  lifecycle.loadError.value = null;
  lifecycle.loadErrorWatchers[0]();
  await flush();
  assert.equal(lifecycle.launchSyncs(), 1);
  assert.equal(lifecycle.probes(), 1);

  // 后续设置变化导致 loadError 再次被置空时不能重复探测。
  lifecycle.loadErrorWatchers[0]();
  await flush();
  assert.equal(lifecycle.launchSyncs(), 1);
  assert.equal(lifecycle.probes(), 1);
});

test("a normal start runs the startup work once and does not repeat it", async () => {
  const lifecycle = setup(null);
  await lifecycle.mounted[0]();
  assert.equal(lifecycle.launchSyncs(), 1);
  assert.equal(lifecycle.probes(), 1);

  lifecycle.loadErrorWatchers[0]();
  await flush();
  assert.equal(lifecycle.launchSyncs(), 1);
  assert.equal(lifecycle.probes(), 1);
});

test("external config reload results and failures reach the controller", async () => {
  const reloaded: unknown[] = [];
  const failed: unknown[] = [];
  const lifecycle = setup(null, {
    onConfigReloaded: (result: unknown) => reloaded.push(result),
    onConfigReloadFailed: (result: unknown) => failed.push(result),
  });
  await lifecycle.mounted[0]();

  lifecycle.listeners.get("config-reloaded")?.({
    payload: {
      path: "/tmp/config.json",
      providerCount: 3,
      settingsChanged: true,
    },
  });
  lifecycle.listeners.get("config-reload-failed")?.({
    payload: { path: "/tmp/config.json", error: "解析失败" },
  });

  assert.deepEqual(reloaded, [
    { path: "/tmp/config.json", providerCount: 3, settingsChanged: true },
  ]);
  assert.deepEqual(failed, [
    { path: "/tmp/config.json", error: "解析失败" },
  ]);
});

test("window visible: provider change events refresh the snapshot right away", async () => {
  let reloads = 0;
  const lifecycle = setup(null, {
    reloadProviders: () => {
      reloads += 1;
    },
  });
  await lifecycle.mounted[0]();

  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  assert.equal(reloads, 1);
  lifecycle.listeners.get("balancehub-events-reconnected")?.({ payload: null });
  assert.equal(reloads, 2);
});

test("window hidden: change events coalesce into one refresh after the window returns", async () => {
  let reloads = 0;
  const lifecycle = setup(null, {
    reloadProviders: () => {
      reloads += 1;
    },
  });
  await lifecycle.mounted[0]();

  // 隐藏到托盘期间没有界面消费快照，事件只记账不拉取。
  lifecycle.doc.visibilityState = "hidden";
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  lifecycle.listeners.get("balancehub-events-reconnected")?.({ payload: null });
  assert.equal(reloads, 0);

  // 仍然隐藏时收到可见性通知也不拉取。
  for (const callback of lifecycle.documentListeners.get("visibilitychange") ?? []) callback();
  assert.equal(reloads, 0);

  // 窗口重新可见后补拉一次，期间积压的多条事件不重复拉取。
  lifecycle.doc.visibilityState = "visible";
  for (const callback of lifecycle.documentListeners.get("visibilitychange") ?? []) callback();
  assert.equal(reloads, 1);

  for (const callback of lifecycle.documentListeners.get("visibilitychange") ?? []) callback();
  assert.equal(reloads, 1);

  // 之后的可见期事件恢复即时刷新。
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  assert.equal(reloads, 2);
});

test("targeted change events refresh only the listed cards", async () => {
  let fullReloads = 0;
  const targeted: string[][] = [];
  const lifecycle = setup(null, {
    reloadProviders: () => {
      fullReloads += 1;
    },
    reloadProviderViews: (ids: string[]) => {
      targeted.push(ids);
    },
  });
  await lifecycle.mounted[0]();

  // 签到、测活这类只影响站点自身的变更带 id，不必重读整份配置。
  lifecycle.listeners.get("providers-changed")?.({
    payload: { providerIds: ["p1", "p2"] },
  });
  await providerFlushDone();
  assert.deepEqual(targeted, [["p1", "p2"]]);

  // 没有 id 的事件（设置、登录账号或整份配置）仍然走完整快照。
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  assert.equal(fullReloads, 1);
  assert.deepEqual(targeted, [["p1", "p2"]]);

  // 载荷格式不符合约定时按完整快照处理，不能静默丢掉变更。
  lifecycle.listeners.get("providers-changed")?.({ payload: { providerIds: "p1" } });
  assert.equal(fullReloads, 2);
});

test("a burst of targeted change events is merged into one view refresh", async () => {
  const targeted: string[][] = [];
  const lifecycle = setup(null, {
    reloadProviders: () => {},
    reloadProviderViews: (ids: string[]) => {
      targeted.push(ids);
    },
  });
  await lifecycle.mounted[0]();
  const notify = lifecycle.listeners.get("providers-changed");
  notify?.({ payload: { providerIds: ["p1"] } });
  notify?.({ payload: { providerIds: ["p2", "p1"] } });
  notify?.({ payload: { providerIds: ["p3"] } });

  // 同一帧内只补拉一次，id 按到达顺序去重。
  assert.deepEqual(targeted, []);
  await providerFlushDone();
  assert.deepEqual(targeted, [["p1", "p2", "p3"]]);
});

test("hidden window merges targeted events and falls back to a full snapshot when needed", async () => {
  let fullReloads = 0;
  const targeted: string[][] = [];
  const lifecycle = setup(null, {
    reloadProviders: () => {
      fullReloads += 1;
    },
    reloadProviderViews: (ids: string[]) => {
      targeted.push(ids);
    },
  });
  await lifecycle.mounted[0]();

  lifecycle.doc.visibilityState = "hidden";
  lifecycle.listeners.get("providers-changed")?.({ payload: { providerIds: ["p1"] } });
  lifecycle.listeners.get("providers-changed")?.({ payload: { providerIds: ["p2"] } });
  lifecycle.listeners.get("providers-changed")?.({ payload: { providerIds: ["p1"] } });
  assert.deepEqual(targeted, []);
  assert.equal(fullReloads, 0);

  lifecycle.doc.visibilityState = "visible";
  for (const callback of lifecycle.documentListeners.get("visibilitychange") ?? []) callback();
  assert.deepEqual(targeted, [["p1", "p2"]]);
  assert.equal(fullReloads, 0);

  // 隐藏期间只要有一条整份变更，恢复后就重读完整快照（定向补拉不够）。
  lifecycle.doc.visibilityState = "hidden";
  lifecycle.listeners.get("providers-changed")?.({ payload: { providerIds: ["p3"] } });
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  lifecycle.doc.visibilityState = "visible";
  for (const callback of lifecycle.documentListeners.get("visibilitychange") ?? []) callback();
  assert.equal(fullReloads, 1);
  assert.deepEqual(targeted, [["p1", "p2"]]);
});

test("window focus flushes a deferred refresh for hosts without visibilitychange", async () => {
  let reloads = 0;
  const lifecycle = setup(null, {
    reloadProviders: () => {
      reloads += 1;
    },
  });
  await lifecycle.mounted[0]();

  lifecycle.doc.visibilityState = "hidden";
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  assert.equal(reloads, 0);

  lifecycle.doc.visibilityState = "visible";
  for (const callback of lifecycle.windowListeners.get("focus") ?? []) callback();
  assert.equal(reloads, 1);
});

test("unmount removes the visibility and focus listeners", async () => {
  let reloads = 0;
  const lifecycle = setup(null, {
    reloadProviders: () => {
      reloads += 1;
    },
  });
  await lifecycle.mounted[0]();
  assert.equal(lifecycle.documentListeners.get("visibilitychange")?.length, 1);
  assert.equal(lifecycle.windowListeners.get("focus")?.length, 1);

  lifecycle.unmounted[0]();

  assert.equal(lifecycle.documentListeners.get("visibilitychange")?.length, 0);
  assert.equal(lifecycle.windowListeners.get("focus")?.length, 0);
  // 卸载后残留的事件回调不能再触发快照拉取。
  lifecycle.listeners.get("providers-changed")?.({ payload: null });
  assert.equal(reloads, 0);
});
