import assert from "node:assert/strict";
import test from "node:test";
import { loadSource } from "./helpers/load-source.ts";

type Callback = () => unknown;

function ref<T>(value: T) {
  return { value };
}

function setup(startupError: string | null) {
  // onMounted 会同步平台样式类，Node 测试环境没有 DOM。
  (globalThis as { document?: unknown }).document = {
    documentElement: {
      classList: { add: () => {}, remove: () => {} },
    },
  };
  const mounted: Callback[] = [];
  const loadErrorWatchers: Callback[] = [];
  let launchSyncs = 0;
  let probes = 0;
  const loadError = ref<string | null>(startupError);
  const { useAppLifecycle } = loadSource<{
    useAppLifecycle: (options: Record<string, unknown>) => void;
  }>("composables/useAppLifecycle.ts", {
    vue: {
      onMounted: (callback: Callback) => mounted.push(callback),
      onUnmounted: () => {},
      watch: (source: unknown, callback: Callback) => {
        if (source === loadError) loadErrorWatchers.push(callback);
      },
    },
    "./useHostPlatform": {
      useHostPlatform: () => ({ ensure: async () => "macos" }),
    },
    "../api/transport.ts": {
      listen: async () => () => {},
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
  });

  return {
    mounted,
    loadError,
    loadErrorWatchers,
    launchSyncs: () => launchSyncs,
    probes: () => probes,
  };
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

test("startup work deferred by a load error runs once after the config is recovered", async () => {
  const lifecycle = setup("解析配置失败");
  await lifecycle.mounted[0]();

  // 启动读取失败时不探测 CLI、不同步开机启动，避免拿默认配置写回系统状态。
  assert.equal(lifecycle.launchSyncs(), 0);
  assert.equal(lifecycle.probes(), 0);

  // 用户修好 data.json 并点击「重新读取」后，loadError 清空。
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
