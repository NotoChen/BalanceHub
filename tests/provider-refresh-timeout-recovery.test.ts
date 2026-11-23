import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import type { Provider } from "../src/stores/provider-types.ts";
import { loadSource } from "./helpers/load-source.ts";

// loadSource 通过 require 加载 pinia；测试里也从 require 取同一份实例，
// 才能让 setActivePinia 作用到被测 store。
const require = createRequire(import.meta.url);
const { createPinia, setActivePinia } = require("pinia") as typeof import("pinia");

type ProviderStore = {
  providers: Provider[];
  refreshingIds: Set<string>;
  providerReloadPending: boolean;
  replaceProviderSnapshot: (providers: Provider[], revision: number) => boolean;
  refreshByIds: (ids: string[]) => Promise<string | null>;
};

function provider(status: Provider["runtime"]["status"], available: number): Provider {
  return {
    identity: { id: "p1", name: "合成站点", baseUrl: "https://fixture.invalid" },
    revision: 1,
    runtime: { enabled: true, status, errorMessage: null },
    quota: { available },
  } as unknown as Provider;
}

const settle = async () => {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
};

test("a refresh that outlives the frontend wait re-reads the backend result", async (t) => {
  let resolveRefresh!: (value: { updatedProviders: Provider[] }) => void;
  const refreshRequest = new Promise<{ updatedProviders: Provider[] }>((resolve) => {
    resolveRefresh = resolve;
  });
  let loadCalls = 0;
  const backend = provider("ok", 88);
  const { useProviderStore } = loadSource<{ useProviderStore: () => ProviderStore }>(
    "stores/providers.ts",
    {
      "../api/app": {
        refreshProviders: () => refreshRequest,
        loadAppData: async () => {
          loadCalls += 1;
          return {
            revision: 2,
            providers: [backend],
            providerProtocols: [],
            settings: {},
            workspaces: [],
            temporaryCliPreferences: [],
          };
        },
      },
      // 后端请求仍然挂着时，前端等待窗口先到期。
      "../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) =>
          Promise.race([
            promise,
            Promise.reject(new Error("刷新等待超时，后台完成后会自动更新")),
          ]),
      },
      "../utils/provider-revision": {
        mergeProvidersByRevision: (current: Provider[], incoming: Provider[]) => {
          const merged = new Map(current.map((item) => [item.identity.id, item]));
          incoming.forEach((item) => merged.set(item.identity.id, item));
          return [...merged.values()];
        },
        pruneProviderTombstones: (_tombstones: unknown, revision: number) => ({ revision }),
      },
      "./cli-runtime": { useCliRuntimeStore: () => ({ refresh: async () => undefined }) },
      "./provider-defaults": { defaultSettings: {} },
      "./settings": { useSettingsStore: () => ({ hydrate: () => undefined }) },
      "./workspaces": {
        useWorkspaceStore: () => ({
          hydrate: () => undefined,
          removeProviderPreference: () => undefined,
        }),
      },
    },
  );

  setActivePinia(createPinia());
  t.after(() => setActivePinia(createPinia()));
  const store = useProviderStore();
  store.replaceProviderSnapshot([provider("ok", 10)], 1);

  const failure = await store.refreshByIds(["p1"]);

  // 超时只代表前端不再等待，不能说明后端失败。
  assert.match(failure ?? "", /刷新等待超时/);
  assert.equal(store.providers[0].runtime.status, "error");
  assert.equal(store.refreshingIds.size, 0);

  // 后端随后完成：请求 promise 落定后必须重新读取，把卡片从误报的
  // 「刷新超时」恢复成真实结果，而不是让错误状态一直留在界面上。
  resolveRefresh({ updatedProviders: [backend] });
  await settle();

  assert.equal(loadCalls, 1);
  assert.equal(store.providers[0].runtime.status, "ok");
  assert.equal(store.providers[0].quota.available, 88);
});
