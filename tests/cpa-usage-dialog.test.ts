import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import type { CpaApiKeyUsage } from "../src/stores/oauth-types.ts";

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
const usage: CpaApiKeyUsage[] = [
  {
    provider: "openai",
    baseUrl: "https://api.example.test",
    key: "sk-first-example",
    success: 8,
    failed: 2,
    recentRequests: [],
  },
  {
    provider: "openai",
    baseUrl: "",
    key: "sk-second",
    success: 1,
    failed: 0,
    recentRequests: [],
  },
  {
    provider: "claude",
    baseUrl: "",
    key: "sk-elsewhere",
    success: 4,
    failed: 1,
    recentRequests: [],
  },
];
function dialog(t: TestContext, read: () => Promise<CpaApiKeyUsage[]>, props?: any) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaUsageDialog.vue",
    {
      "@lucide/vue": { Copy: {}, Eye: {}, EyeOff: {}, KeyRound: {} },
      "@arco-design/web-vue": { Message: { success() {} } },
      "../../utils/oauth-display": { quotaNumber: (value: number) => `${value}` },
      "../../composables/useClipboard": { copyText: async () => undefined },
      "../../api/oauth-accounts": { readCpaApiKeyUsage: read },
    },
  );
  return mount(t, component, props ?? { visible: true, provider: provider() });
}

test("usage is grouped by provider and totals stay per row", async (t) => {
  const { state } = dialog(t, async () => usage);
  await settle();
  assert.equal(state.totals.value.keys, 3);
  assert.equal(state.totals.value.success, 13);
  assert.equal(state.totals.value.failed, 3);
  assert.deepEqual(
    state.groups.value.map((group: any) => [
      group.provider,
      group.success,
      group.failed,
    ]),
    [
      ["openai", 9, 2],
      ["claude", 4, 1],
    ],
  );
  // A key stays masked until its own row asks for it.
  assert.equal(state.revealed.value, "");
  state.toggleReveal(usage[0]);
  assert.equal(state.revealed.value, "openai|sk-first-example");
  state.toggleReveal(usage[0]);
  assert.equal(state.revealed.value, "");
});

test("the failure meter fills from the share of failed requests", async (t) => {
  const { state } = dialog(t, async () => usage);
  await settle();
  assert.equal(state.failedShare(8, 2), 20);
  // A credential with no traffic has no failure share rather than a full bar.
  assert.equal(state.failedShare(0, 0), 0);
});

test("a gateway with nothing to report says where the counts come from", async (t) => {
  const { state } = dialog(t, async () => []);
  await settle();
  assert.deepEqual(state.usage.value, []);
  assert.deepEqual(state.groups.value, []);
  assert.equal(state.totals.value.keys, 0);
});

test("closing mid-read releases the busy state and drops the late answer", async (t) => {
  let release!: (value: CpaApiKeyUsage[]) => void;
  const pending = new Promise<CpaApiKeyUsage[]>((resolve) => {
    release = resolve;
  });
  const props = reactive({ visible: true, provider: provider() });
  const { state } = dialog(t, () => pending, props);
  await settle();
  assert.equal(state.loading.value, true);

  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false);
  release(usage);
  await settle();
  assert.equal(state.usage.value, null);
});
