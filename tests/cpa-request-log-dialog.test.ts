import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import { cpaLogLevels, parseCpaLogLine } from "../src/utils/cpa-log.ts";

type SetupComponent = {
  setup: (props: any, context: any) => Record<string, any>;
};
function mount(t: TestContext, component: SetupComponent, props: any = {}) {
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
  let state!: Record<string, any>;
  const app = renderer.createApp({
    setup() {
      state = component.setup(props, { expose() {}, emit() {} });
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
  return { state, dispose };
}
async function settle() {
  for (let i = 0; i < 16; i++) await Promise.resolve();
  await nextTick();
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function provider(id = "oauth-fixture"): Provider {
  return {
    identity: { id, name: id, remark: "" },
    auth: { credentialRevision: 1, mode: "accessToken" },
    capabilities: { subscription: null, availableModels: [] },
    automation: { refreshInterval: 0 },
    runtime: { enabled: true },
    actions: { oauthAccount: true, cpaGateway: true },
  } as unknown as Provider;
}
const tail = {
  lines: [
    "[2026-10-22 10:00:00] [ab12cd34] [info ] [server.go:42] upstream request completed latency=120ms",
    "[2026-10-22 10:00:01] [--------] [error] [auth.go:9] refresh failed",
  ],
  lineCount: 2,
  latestAt: 1_761_117_600,
  nextCursor: "cursor-1",
  cursorReset: false,
};
type Bindings = Record<string, unknown>;
function dialog(t: TestContext, bindings: Bindings = {}, props?: any) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaRequestLogDialog.vue",
    {
      "@lucide/vue": { Pause: {}, Play: {}, RefreshCw: {}, Trash2: {} },
      "@arco-design/web-vue": {},
      "../../utils/oauth-display": { accountTimestamp: () => "" },
      "../../utils/cpa-log": { parseCpaLogLine, cpaLogLevels },
      "../../composables/provider-credential-dialogs": {
        confirmAction: async () => true,
      },
      "../../api/oauth-accounts": {
        getCpaLogSettings: async () => ({ enabled: true }),
        readCpaRequestLogs: async () => tail,
        readCpaRequestLog: async (_id: string, requestId: string) => ({
          requestId,
          content: "one request",
        }),
        clearCpaLogs: async () => ({ removed: 0 }),
        setCpaLogSettings: async () => undefined,
      },
      ...bindings,
    },
  );
  // A spread would copy a reactive props object into a plain one and stop the
  // invisible/visible watcher from firing.
  return mount(t, component, props ?? { visible: true, provider: provider() });
}

test("a log line exposes the request it belongs to as a drill-down", async (t) => {
  const reads: string[] = [];
  const { state } = dialog(t, {
    "../../api/oauth-accounts": {
      getCpaLogSettings: async () => ({ enabled: true }),
      readCpaRequestLogs: async () => tail,
      readCpaRequestLog: async (_id: string, requestId: string) => {
        reads.push(requestId);
        return { requestId, content: "one request" };
      },
      clearCpaLogs: async () => ({ removed: 0 }),
      setCpaLogSettings: async () => undefined,
    },
  });
  await settle();
  const [first, second] = state.parsed.value;
  assert.equal(first.requestId, "ab12cd34");
  assert.equal(first.level, "info");
  assert.equal(first.source, "server.go:42");
  // A line without a request still carries eight dashes, which is not an ID.
  assert.equal(second.requestId, "");

  await state.openRequest(first.requestId);
  assert.deepEqual(reads, ["ab12cd34"]);
  assert.equal(state.requestLog.value.content, "one request");
  assert.equal(state.readingRequest.value, false);
});

test("the level and text filters narrow the buffer without another read", async (t) => {
  let reads = 0;
  const { state } = dialog(t, {
    "../../api/oauth-accounts": {
      getCpaLogSettings: async () => ({ enabled: true }),
      readCpaRequestLogs: async () => {
        reads++;
        return tail;
      },
      readCpaRequestLog: async () => ({ requestId: "", content: "" }),
      clearCpaLogs: async () => ({ removed: 0 }),
      setCpaLogSettings: async () => undefined,
    },
  });
  await settle();
  assert.deepEqual(state.levels.value, ["error", "info"]);
  assert.equal(state.shown.value.length, 2);

  state.levelFilter.value = "error";
  assert.equal(state.shown.value.length, 1);
  assert.equal(state.shown.value[0].level, "error");

  state.levelFilter.value = "all";
  state.filter.value = "LATENCY";
  assert.equal(state.shown.value.length, 1);
  assert.match(state.shown.value[0].raw, /latency=120ms/);
  assert.equal(reads, 1);
});

test("clearing the log asks first, reports what went away and reloads the tail", async (t) => {
  const confirmations: { title: string; description: string }[] = [];
  const calls: [string, number][] = [];
  let reads = 0;
  const { state } = dialog(t, {
    "../../composables/provider-credential-dialogs": {
      confirmAction: async (title: string, description: string) => {
        confirmations.push({ title, description });
        return true;
      },
    },
    "../../api/oauth-accounts": {
      getCpaLogSettings: async () => ({ enabled: true }),
      readCpaRequestLogs: async () => {
        reads++;
        return tail;
      },
      readCpaRequestLog: async () => ({ requestId: "", content: "" }),
      clearCpaLogs: async (id: string, revision: number) => {
        calls.push([id, revision]);
        return { removed: 3 };
      },
      setCpaLogSettings: async () => undefined,
    },
  });
  await settle();
  assert.equal(reads, 1);

  await state.clearLogs();
  await settle();
  assert.equal(confirmations.length, 1);
  // The confirmation has to say that rotated files are deleted, because they
  // do not come back.
  assert.match(confirmations[0].description, /无法恢复/);
  assert.deepEqual(calls, [["oauth-fixture", 1]]);
  assert.match(state.notice.value, /3 个已轮转的日志文件/);
  assert.equal(state.clearing.value, false);
  assert.equal(reads, 2);
});

test("closing mid-clear releases the busy state and drops the late result", async (t) => {
  const clearing = deferred<{ removed: number }>();
  const props = reactive({ visible: true, provider: provider() });
  const { state } = dialog(
    t,
    {
      "../../api/oauth-accounts": {
        getCpaLogSettings: async () => ({ enabled: true }),
        readCpaRequestLogs: async () => tail,
        readCpaRequestLog: async () => ({ requestId: "", content: "" }),
        clearCpaLogs: async () => clearing.promise,
        setCpaLogSettings: async () => undefined,
      },
    },
    props,
  );
  await settle();
  const running = state.clearLogs();
  await settle();
  assert.equal(state.clearing.value, true);

  props.visible = false;
  await settle();
  assert.equal(state.clearing.value, false);
  clearing.resolve({ removed: 4 });
  await running;
  await settle();
  assert.equal(state.notice.value, "");
  assert.equal(state.clearing.value, false);
});
