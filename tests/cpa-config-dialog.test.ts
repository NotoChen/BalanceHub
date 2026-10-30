import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import type { CpaGatewayConfig } from "../src/stores/oauth-types.ts";

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
    auth: { credentialRevision: 3, mode: "accessToken" },
    capabilities: { subscription: null, availableModels: [] },
    automation: { refreshInterval: 0 },
    runtime: { enabled: true },
    actions: { oauthAccount: true, cpaGateway: true },
  } as unknown as Provider;
}
const document: CpaGatewayConfig = {
  content: "routing:\n  strategy: round-robin\n",
  etag: '"hash-1"',
  apiVersion: "v8",
};
function dialog(
  t: TestContext,
  bindings: Record<string, unknown> = {},
  props?: any,
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaConfigDialog.vue",
    {
      "@arco-design/web-vue": { Message: { success() {} } },
      "../ContentEditor.vue": {},
      "../../api/oauth-accounts": {
        readCpaGatewayConfig: async () => document,
        updateCpaGatewayConfig: async () => undefined,
      },
      ...bindings,
    },
  );
  return mount(
    t,
    component,
    props ?? { visible: true, provider: provider(), revision: 3 },
  );
}

test("the document loads into the editor and a save carries the revision tag", async (t) => {
  const writes: any[][] = [];
  const { state } = dialog(t, {
    "../../api/oauth-accounts": {
      readCpaGatewayConfig: async () => document,
      updateCpaGatewayConfig: async (...args: any[]) => {
        writes.push(args);
      },
    },
  });
  await settle();
  assert.equal(state.config.value.content, document.content);
  assert.equal(state.draft.value, document.content);
  assert.equal(state.dirty.value, false);

  state.draft.value = `${document.content}debug: true\n`;
  assert.equal(state.dirty.value, true);
  await state.save();
  // The tag travels with the text, so a concurrent edit is refused instead of
  // being replaced; the revision keeps the panel bound to this connection.
  assert.deepEqual(writes, [
    [
      "oauth-fixture",
      `${document.content}debug: true\n`,
      '"hash-1"',
      "v8",
      3,
    ],
  ]);
  // The gateway rewrites the layout, so the editor reloads the saved file.
  assert.equal(state.draft.value, document.content);
  assert.equal(state.dirty.value, false);
});

test("a refused save keeps the draft and names the gateway's reason", async (t) => {
  const { state } = dialog(t, {
    "../../api/oauth-accounts": {
      readCpaGatewayConfig: async () => document,
      updateCpaGatewayConfig: async () => {
        throw "配置不是有效的 YAML：line 3: did not find expected key";
      },
    },
  });
  await settle();
  state.draft.value = "routing: [\n";
  await state.save();
  assert.equal(
    state.error.value,
    "配置不是有效的 YAML：line 3: did not find expected key",
  );
  // Nothing was accepted, so nothing is discarded or reloaded.
  assert.equal(state.draft.value, "routing: [\n");
  assert.equal(state.dirty.value, true);
});

test("the document is read again after a save, not reused", async (t) => {
  let reads = 0;
  const { state } = dialog(t, {
    "../../api/oauth-accounts": {
      readCpaGatewayConfig: async () =>
        reads++ === 0
          ? document
          : { ...document, content: "routing:\n  strategy: fill-first\n", etag: '"hash-2"' },
      updateCpaGatewayConfig: async () => undefined,
    },
  });
  await settle();
  state.draft.value = "changed: true\n";
  await state.save();
  assert.equal(state.draft.value, "routing:\n  strategy: fill-first\n");
  assert.equal(state.config.value.etag, '"hash-2"');
});

test("closing mid-read releases the busy state and drops the late answer", async (t) => {
  let release!: (value: CpaGatewayConfig) => void;
  const pending = new Promise<CpaGatewayConfig>((resolve) => {
    release = resolve;
  });
  const props = reactive({ visible: true, provider: provider(), revision: 3 });
  const { state } = dialog(
    t,
    {
      "../../api/oauth-accounts": {
        readCpaGatewayConfig: () => pending,
        updateCpaGatewayConfig: async () => undefined,
      },
    },
    props,
  );
  await settle();
  assert.equal(state.loading.value, true);

  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false);
  release(document);
  await settle();
  assert.equal(state.config.value, null);
});
