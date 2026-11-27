import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";

type SetupState = Record<string, any>;

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

const completedTask = {
  id: "login-1",
  phase: "completed",
  providerId: "provider-a",
  vendor: "codex",
  message: "账号已保存",
  createdAt: 1_700_000_000,
  updatedAt: 1_700_000_100,
};

test("a failing provider refresh reports once but keeps retrying quietly", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const messages: string[] = [];
  const store = reactive<Record<string, any>>({
    tasks: {},
    visible: true,
    eventError: "",
    accept(task: typeof completedTask) {
      this.tasks[task.id] = task;
    },
    openAccount() {},
    openTask() {},
  });
  let reloads = 0;
  let failing = true;
  const provider = {
    identity: { id: "provider-a" },
    automation: { lastSyncedAt: "2026-11-01T00:00:00Z" },
  };
  const providers = {
    providers: [provider],
    reloadProvider: async (id: string) => {
      reloads += 1;
      if (failing) throw new Error("读取配置失败");
      return id === provider.identity.id ? provider : null;
    },
    refreshByIds: async () => null,
  };
  const { useOAuthTasks } = loadSource<{
    useOAuthTasks: () => SetupState;
  }>("composables/useOAuthTasks.ts", {
    "../api/transport.ts": { listen: async () => () => {} },
    "@arco-design/web-vue": {
      Message: {
        error: (message: unknown) => messages.push(String(message)),
      },
    },
    "../stores/oauth-accounts": { useOAuthAccountStore: () => store },
    "../stores/providers": { useProviderStore: () => providers },
    "../api/oauth-accounts": {
      listOAuthLogins: async () => [completedTask],
      cancelOAuthLogin: async () => {},
      retryOAuthSave: async () => {},
    },
    "../utils/promise-timeout": { withTimeout: (promise: Promise<unknown>) => promise },
    "../utils/oauth-display": { oauthVendorLabel: () => "Codex" },
  });
  mount(t, () => useOAuthTasks());
  await settle();
  assert.deepEqual(messages, ["Error: 读取配置失败"]);
  assert.equal(reloads, 1);
  assert.ok(store.tasks["login-1"], "完成的任务仍然进入后台任务列表");

  // 下一次轮询会安静重试：不再重复同一条弹窗，但列表仍有机会恢复。
  t.mock.timers.tick(15_000);
  await settle();
  assert.equal(reloads, 2);
  assert.deepEqual(messages, ["Error: 读取配置失败"]);

  failing = false;
  t.mock.timers.tick(15_000);
  await settle();
  assert.equal(reloads, 3);
  assert.deepEqual(messages, ["Error: 读取配置失败"], "重试成功后不再提示旧失败");

  // 成功之后任务已处理完，后续轮询不会再重复读取。
  t.mock.timers.tick(15_000);
  await settle();
  assert.equal(reloads, 3);
});

test("a backend restart releases waiting OAuth tasks instead of polling forever", async (t) => {
  const nowSeconds = Math.floor(Date.now() / 1000);
  const stale = {
    id: "stale",
    phase: "waiting",
    providerId: null,
    vendor: "linuxdo",
    message: "等待授权",
    revision: 3,
    createdAt: 1_700_000_000,
    updatedAt: 1_700_000_100,
    canCancel: true,
    canRetrySave: false,
  };
  const fresh = {
    ...stale,
    id: "fresh",
    createdAt: nowSeconds,
    updatedAt: nowSeconds,
  };
  const store = reactive<Record<string, any>>({
    tasks: { stale, fresh },
    visible: false,
    eventError: "",
    accept(task: typeof stale) {
      const previous = this.tasks[task.id];
      if (!previous || previous.revision <= task.revision) this.tasks[task.id] = task;
    },
    openAccount() {},
    openTask() {},
  });
  const { useOAuthTasks } = loadSource<{
    useOAuthTasks: () => SetupState;
  }>("composables/useOAuthTasks.ts", {
    "../api/transport.ts": { listen: async () => () => {} },
    "@arco-design/web-vue": { Message: { error() {} } },
    "../stores/oauth-accounts": { useOAuthAccountStore: () => store },
    "../stores/providers": { useProviderStore: () => ({ providers: [], reloadProvider: async () => null, refreshByIds: async () => null }) },
    "../api/oauth-accounts": {
      // 重启后的后端只返回内存里现存的授权任务。
      listOAuthLogins: async () => [],
      cancelOAuthLogin: async () => {},
      retryOAuthSave: async () => {},
    },
    "../utils/promise-timeout": { withTimeout: (promise: Promise<unknown>) => promise },
    "../utils/oauth-display": { oauthVendorLabel: () => "Linux DO" },
  });
  mount(t, () => useOAuthTasks());
  await settle();
  assert.equal(store.tasks.stale.phase, "failed");
  assert.match(store.tasks.stale.message, /后台列表消失/);
  assert.equal(store.tasks.stale.canCancel, false);
  assert.equal(store.tasks.fresh.phase, "waiting", "刚发起的授权不会被误判为已消失");
});
