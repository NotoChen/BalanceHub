import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import type { Provider } from "../src/stores/provider-types.ts";
import { sameActiveEndpoint } from "../src/utils/provider-revision.ts";
import { loadSource } from "./helpers/load-source.ts";

// loadSource 通过 require 加载 pinia；测试里也从 require 取同一份实例，
// 才能让 setActivePinia 作用到被测 store。
const require = createRequire(import.meta.url);
const { createPinia, setActivePinia } = require("pinia") as typeof import("pinia");

type AppData = {
  revision: number;
  providers: Provider[];
  providerProtocols: unknown[];
  settings: Record<string, unknown>;
  workspaces: unknown[];
  temporaryCliPreferences: unknown[];
};

type ProviderStore = {
  providers: Provider[];
  providerSnapshotRevision: number;
  providerReloadPending: boolean;
  refreshInProgress: boolean;
  reload: () => Promise<void>;
  reloadProviders: () => Promise<void>;
  reloadProvider: (id: string) => Promise<Provider | null>;
  reloadProviderViews: (ids: string[]) => Promise<Provider[]>;
  replaceProviderSnapshot: (providers: Provider[], revision: number) => boolean;
  upsertProviders: (providers: Provider[]) => void;
};

function provider(overrides: { id?: string; available?: number; revision?: number } = {}): Provider {
  return {
    identity: { id: overrides.id ?? "p1", name: "合成站点", baseUrl: "https://fixture.invalid" },
    revision: overrides.revision ?? 1,
    runtime: { enabled: true, status: "ok", errorMessage: null },
    quota: { available: overrides.available ?? 10 },
  } as unknown as Provider;
}

function appData(revision: number, providers: Provider[]): AppData {
  return {
    revision,
    providers,
    providerProtocols: [],
    settings: {},
    workspaces: [],
    temporaryCliPreferences: [],
  };
}

const settle = async () => {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
};

type ProviderViews = { revision: number; providers: Provider[] };

function loadStore(
  loadAppData: () => Promise<AppData>,
  loadProviderViews: (ids: string[]) => Promise<ProviderViews> = async () => {
    throw new Error("未预期的单卡刷新");
  },
) {
  const { useProviderStore } = loadSource<{ useProviderStore: () => ProviderStore }>(
    "stores/providers.ts",
    {
      "../api/app": { loadAppData, loadProviderViews },
      "../utils/promise-timeout": {
        withTimeout: (promise: Promise<unknown>) => promise,
      },
      "../utils/provider-revision": {
        mergeProvidersByRevision: (current: Provider[], incoming: Provider[]) => {
          const merged = new Map(current.map((item) => [item.identity.id, item]));
          incoming.forEach((item) => merged.set(item.identity.id, item));
          return [...merged.values()];
        },
        pruneProviderTombstones: (_tombstones: unknown, revision: number) => ({ revision }),
        sameActiveEndpoint,
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
  return useProviderStore();
}

test("并发快照请求共用一次读取，并在结束后补拉一次最新状态", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  const requests: Array<(data: AppData) => void> = [];
  let loadCalls = 0;
  const store = loadStore(() => new Promise<AppData>((resolve) => {
    loadCalls += 1;
    requests.push(resolve);
  }));

  const first = store.reload();
  const second = store.reload();
  const third = store.reloadProviders();
  await settle();

  // 后台一轮批量变更连发多条事件时，同一时刻只能有一个完整快照在飞。
  assert.equal(loadCalls, 1);

  requests[0](appData(1, [provider()]));
  await settle();

  // 在飞期间到达的请求已经错过这一轮，必须补拉一次，不能直接丢。
  assert.equal(loadCalls, 2);
  assert.equal(store.providerReloadPending, false);

  requests[1](appData(2, [provider({ available: 42 })]));
  await Promise.all([first, second, third]);

  assert.equal(loadCalls, 2);
  assert.equal(store.providerSnapshotRevision, 2);
  assert.equal(store.providers[0].quota.available, 42);
});

test("刷新批次进行中到达的快照请求留给批次结束后的补拉", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  let loadCalls = 0;
  const store = loadStore(async () => {
    loadCalls += 1;
    return appData(1, [provider()]);
  });

  store.refreshInProgress = true;
  await store.reload();
  await store.reloadProviders();

  // 批量刷新期间不并发读取：结果由刷新命令自己写回，结束后统一补拉。
  assert.equal(loadCalls, 0);
  assert.equal(store.providerReloadPending, true);
});

test("内容没变的中转站复用旧对象，卡片不会被整块看板刷新拖着重渲染", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  const requests: Array<(data: AppData) => void> = [];
  const store = loadStore(() => new Promise<AppData>((resolve) => {
    requests.push(resolve);
  }));

  store.replaceProviderSnapshot([provider(), provider({ id: "p2" })], 1);
  const untouched = [store.providers[0], store.providers[1]];

  const reload = store.reload();
  await settle();
  // 新快照是后端重新序列化的结果：revision 是全局版本戳，两个站点都会
  // 推进到 2；p1 的内容没变，p2 的额度变了。
  requests[0](appData(2, [
    provider({ revision: 2 }),
    provider({ id: "p2", available: 99, revision: 2 }),
  ]));
  await reload;

  assert.equal(store.providers[0], untouched[0]);
  // 版本号仍要跟上本轮快照，否则晚到的旧响应会被误判成更新。
  assert.equal(store.providers[0].revision, 2);
  assert.notEqual(store.providers[1], untouched[1]);
  assert.equal(store.providers[1].quota.available, 99);
});

test("整份快照都没有实质变化时连数组引用也不换", (t) => {
  t.after(() => setActivePinia(createPinia()));
  const store = loadStore(async () => appData(1, []));
  store.replaceProviderSnapshot([provider(), provider({ id: "p2" })], 1);
  const snapshot = store.providers;

  store.replaceProviderSnapshot(
    [provider({ revision: 2 }), provider({ id: "p2", revision: 2 })],
    2,
  );

  assert.equal(store.providers, snapshot);
  assert.equal(store.providerSnapshotRevision, 2);
});

test("单卡刷新只取这张卡片的视图，不再读取整份配置", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  let fullLoads = 0;
  const requested: string[][] = [];
  const store = loadStore(
    async () => {
      fullLoads += 1;
      return appData(1, []);
    },
    async (ids) => {
      requested.push(ids);
      return { revision: 3, providers: [provider({ revision: 3, available: 77 })] };
    },
  );
  store.replaceProviderSnapshot([provider(), provider({ id: "p2" })], 1);

  const updated = await store.reloadProvider("p1");

  assert.deepEqual(requested, [["p1"]]);
  assert.equal(fullLoads, 0);
  assert.equal(updated?.quota.available, 77);
  assert.equal(store.providers[0].quota.available, 77);
});

test("一批卡片动作合并成一次定向读取，缺失的站点按快照版本判定", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  const requested: string[][] = [];
  const store = loadStore(
    async () => appData(1, []),
    async (ids) => {
      requested.push(ids);
      return {
        revision: 6,
        providers: [provider({ id: "p1", revision: 6, available: 55 })],
      };
    },
  );
  store.replaceProviderSnapshot(
    [provider({ revision: 6 }), provider({ id: "p2", revision: 6 }), provider({ id: "p3", revision: 6 })],
    6,
  );

  const refreshed = await store.reloadProviderViews(["p1", "p2", "p2"]);

  // 同一批动作只发一次 IPC，重复 id 去重。
  assert.deepEqual(requested, [["p1", "p2"]]);
  assert.equal(refreshed.length, 1);
  assert.equal(store.providers.find((item) => item.identity.id === "p1")?.quota.available, 55);
  // p2 缺失且响应不早于快照版本：后端确实已经没有这个站点，卡片移除；
  // 未被请求的 p3 不受影响。
  assert.equal(store.providers.some((item) => item.identity.id === "p2"), false);
  assert.equal(store.providers.some((item) => item.identity.id === "p3"), true);
  assert.equal(store.providerTombstones.p2, 6);
});

test("只有不早于完整快照的响应才能判定站点已删除", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  const responses: ProviderViews[] = [
    // 完整快照还没包含的晚到旧视图：不能据此删掉当前卡片。
    { revision: 4, providers: [] },
    { revision: 5, providers: [] },
  ];
  const store = loadStore(
    async () => appData(1, []),
    async () => responses.shift()!,
  );
  store.replaceProviderSnapshot([provider({ revision: 5 })], 5);

  await store.reloadProvider("p1");
  assert.equal(store.providers.length, 1);

  await store.reloadProvider("p1");
  assert.equal(store.providers.length, 0);
});

test("定向读取推进版本后，晚到的旧完整快照不会把卡片回滚", (t) => {
  t.after(() => setActivePinia(createPinia()));
  const store = loadStore(async () => appData(1, []));
  store.replaceProviderSnapshot(
    [provider({ revision: 5 }), provider({ id: "p2", revision: 5 })],
    5,
  );

  // 一次卡片动作只回传 p1 的新视图；整份快照的版本号还停在 5。
  store.upsertProviders([provider({ revision: 7, available: 77 })]);
  assert.equal(store.providers[0].revision, 7);

  // 早于定向结果的完整快照不能覆盖新读数，也不能据此删掉新站点。
  const accepted = store.replaceProviderSnapshot(
    [provider({ revision: 5 }), provider({ id: "p2", revision: 5 })],
    5,
  );
  assert.equal(accepted, false);
  assert.equal(store.providers[0].quota.available, 77);
  assert.equal(store.providerSnapshotRevision, 5);
});

test("版本号相同的重复快照只跟进备用地址读数", (t) => {
  t.after(() => setActivePinia(createPinia()));
  const store = loadStore(async () => appData(1, []));
  const snapshotProvider = (endpoint?: Provider["activeEndpoint"]) => {
    const value = provider({ revision: 3 });
    value.activeEndpoint = endpoint;
    return value;
  };
  store.replaceProviderSnapshot(
    [snapshotProvider(), provider({ id: "p2", revision: 3 })],
    3,
  );
  const snapshot = store.providers;
  const first = snapshot[0];
  const second = snapshot[1];

  // 同版本号的重复快照内容与备用地址都没变：数组和对象都保持原身份。
  store.replaceProviderSnapshot(
    [snapshotProvider(), provider({ id: "p2", revision: 3 })],
    3,
  );
  assert.equal(store.providers, snapshot);

  // 只有备用地址切换：替换这一个站点，其余保持原对象。
  store.replaceProviderSnapshot(
    [
      snapshotProvider({ backupUrl: "https://backup.invalid", backupIndex: 1, switchedAt: 9 }),
      provider({ id: "p2", revision: 3 }),
    ],
    3,
  );
  assert.notEqual(store.providers, snapshot);
  assert.notEqual(store.providers[0], first);
  assert.equal(store.providers[1], second);
  assert.equal(store.providers[0].activeEndpoint?.backupIndex, 1);
});

test("设置快照内容没变时保留原引用，避免外壳与卡片跟着换 props", async (t) => {
  t.after(() => setActivePinia(createPinia()));
  type SettingsStore = {
    settings: Record<string, unknown>;
    hydrate: (settings: Record<string, unknown>) => void;
  };
  const { useSettingsStore } = loadSource<{ useSettingsStore: () => SettingsStore }>(
    "stores/settings.ts",
    {
      "../api/app": { saveSettings: async () => ({}) },
      "./provider-defaults": { defaultSettings: () => ({ themeMode: "light" }) },
    },
  );
  setActivePinia(createPinia());
  const store = useSettingsStore();

  store.hydrate({ themeMode: "light" });
  const original = store.settings;

  store.hydrate({ themeMode: "light" });
  assert.equal(store.settings, original);

  store.hydrate({ themeMode: "dark" });
  assert.notEqual(store.settings, original);
  assert.equal(store.settings.themeMode, "dark");
});
