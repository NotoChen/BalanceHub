import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick } from "vue";
import type { BrowserRuntimeStatus } from "../src/api/browser-runtime.ts";
import { createBrowserRuntimeController } from "../src/utils/browser-runtime-controller.ts";
import { loadSource } from "./helpers/load-source.ts";

function mount(t: TestContext, setup: () => unknown) {
  const renderer = createRenderer<object, object>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: () => ({}),
    createText: () => ({}),
    createComment: () => ({}),
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  const app = renderer.createApp({
    setup() {
      setup();
      return () => null;
    },
  });
  app.mount({});
  let active = true;
  const dispose = () => {
    if (active) {
      active = false;
      app.unmount();
    }
  };
  t.after(dispose);
  return { dispose };
}

async function settle() {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
  await nextTick();
}

function status(values: Partial<BrowserRuntimeStatus>): BrowserRuntimeStatus {
  return {
    phase: "notInstalled", message: "浏览器辅助组件尚未安装", ready: false, installed: false,
    browser: null, systemBrowsers: [], selection: { mode: "managed" }, runtimeReady: false,
    expectedVersion: "fixture", installedVersion: null, progress: null, revision: 1,
    detectedAt: 1_000, canInstall: true, canUninstall: false, runtimeDownloadBytes: 1, browserDownloadBytes: 1,
    ...values,
  };
}

test("a reconnected event stream re-reads the component status instead of keeping a stopped install", async (t) => {
  const handlers = new Map<string, (payload: unknown) => void>();
  let reads = 0;
  // 后端重启前停在 50% 的安装；重启后状态从头计数，组件目录只剩半成品。
  let current = status({ phase: "installing", message: "正在下载独立 Chromium 浏览器", progress: 0.5, revision: 50, detectedAt: 1_000 });
  const { useBrowserRuntime } = loadSource<{ useBrowserRuntime: () => Record<string, any> }>(
    "composables/useBrowserRuntime.ts",
    {
      "../api/transport.ts": {
        listen: async (event: string, callback: (payload: unknown) => void) => {
          handlers.set(event, callback);
          return () => { handlers.delete(event); };
        },
      },
      "./useHostCapabilities": { supportsCommand: () => true },
      "../utils/browser-runtime-controller": { createBrowserRuntimeController },
      "../api/browser-runtime": {
        getBrowserRuntimeStatus: async () => { reads += 1; return current; },
        listenBrowserRuntime: async () => () => {},
        installBrowserRuntime: async () => current,
        selectBrowserRuntimeBrowser: async () => current,
        cancelBrowserRuntimeInstall: async () => {},
        uninstallBrowserRuntime: async () => current,
      },
    },
  );
  let runtime!: Record<string, any>;
  const mounted = mount(t, () => { runtime = useBrowserRuntime(); });
  await settle();
  assert.equal(reads, 1, "挂载时读取一次状态");
  assert.equal(runtime.state.value?.phase, "installing");
  assert.equal(handlers.has("balancehub-events-reconnected"), true);

  // 后端重启：新进程的版本号从头开始，但检测时间是新进程的墙钟。
  current = status({ phase: "needsCheck", message: "已找到浏览器，完成启动验证后即可使用", revision: 1, detectedAt: 2_000 });
  handlers.get("balancehub-events-reconnected")?.(null);
  await settle();
  assert.equal(reads, 2, "重连后必须补读状态");
  assert.equal(runtime.state.value?.phase, "needsCheck", "重启后的状态必须替换掉中断的安装");
  mounted.dispose();
  assert.equal(handlers.has("balancehub-events-reconnected"), false, "卸载后不再响应重连");
});
