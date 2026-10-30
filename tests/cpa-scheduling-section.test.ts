import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import type {
  CpaGatewaySettings,
  CpaGatewaySettingsUpdate,
} from "../src/stores/oauth-types.ts";

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
const saved: CpaGatewaySettings = {
  routingStrategy: "round-robin",
  proxyUrl: "",
  requestRetry: 2,
  maxRetryCredentials: null,
  maxRetryInterval: null,
  switchProject: true,
  switchPreviewModel: false,
  requestLog: false,
  logsMaxTotalSizeMb: 300,
  errorLogsMaxFiles: 10,
  usageStatisticsEnabled: true,
  debug: false,
  forceModelPrefix: true,
  wsAuth: null,
};
function section(
  t: TestContext,
  bindings: Record<string, unknown> = {},
  props: Record<string, unknown> = { provider: provider(), revision: 1 },
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaSchedulingSection.vue",
    {
      "@lucide/vue": { ChevronDown: {} },
      "@arco-design/web-vue": { Message: { success() {} } },
      "./CpaConfigDialog.vue": {},
      "../../api/oauth-accounts": {
        getCpaGatewaySettings: async () => saved,
        updateCpaGatewaySettings: async () => undefined,
      },
      ...bindings,
    },
  );
  return mount(t, component, props);
}
/// Reads the gateway settings the way the panel does when it is first opened.
async function open(state: Record<string, any>) {
  state.toggleSettings();
  await settle();
}

test("gateway switches load into the draft and only a changed switch is sent", async (t) => {
  const writes: CpaGatewaySettingsUpdate[] = [];
  const { state } = section(t, {
    "../../api/oauth-accounts": {
      getCpaGatewaySettings: async () => saved,
      updateCpaGatewaySettings: async (_id: string, patch: CpaGatewaySettingsUpdate) => {
        writes.push(patch);
      },
    },
  });
  await open(state);
  const draft = state.settingsDraft.value;
  assert.equal(draft.switchProject, true);
  assert.equal(draft.switchPreviewModel, false);
  assert.equal(draft.requestLog, false);
  assert.equal(draft.usageStatisticsEnabled, true);
  assert.equal(draft.debug, false);
  assert.equal(draft.forceModelPrefix, true);
  assert.equal(draft.logsMaxTotalSizeMb, 300);
  assert.equal(draft.errorLogsMaxFiles, 10);
  // Reading the settings must not look like an edit.
  assert.equal(state.settingsDirty.value, false);

  draft.switchPreviewModel = true;
  assert.deepEqual(state.settingsPatch.value, { switchPreviewModel: true });
  await state.saveSettings();
  assert.deepEqual(writes, [{ switchPreviewModel: true }]);
  // The confirmed value replaces the draft, so the panel is clean again.
  assert.equal(state.settingsDirty.value, false);
  assert.equal(state.settingsDraft.value.switchPreviewModel, true);
});

test("a switch the gateway did not report stays untouched until it is flipped", async (t) => {
  const writes: CpaGatewaySettingsUpdate[] = [];
  const { state } = section(t, {
    "../../api/oauth-accounts": {
      getCpaGatewaySettings: async () => ({ ...saved, requestLog: null }),
      updateCpaGatewaySettings: async (_id: string, patch: CpaGatewaySettingsUpdate) => {
        writes.push(patch);
      },
    },
  });
  await open(state);
  const draft = state.settingsDraft.value;
  assert.equal(draft.requestLog, false);
  assert.equal(state.settingsDirty.value, false);

  draft.requestLog = true;
  assert.deepEqual(state.settingsPatch.value, { requestLog: true });
  await state.saveSettings();
  assert.deepEqual(writes, [{ requestLog: true }]);
});

test("the routing and debug switches are written on their own", async (t) => {
  const writes: CpaGatewaySettingsUpdate[] = [];
  const { state } = section(t, {
    "../../api/oauth-accounts": {
      getCpaGatewaySettings: async () => saved,
      updateCpaGatewaySettings: async (_id: string, patch: CpaGatewaySettingsUpdate) => {
        writes.push(patch);
      },
    },
  });
  await open(state);
  const draft = state.settingsDraft.value;
  // An unreported WebSocket switch reads as off in the draft, but reading it
  // back is not an edit, so opening the section sends nothing.
  assert.equal(draft.wsAuth, false);
  assert.equal(state.settingsDirty.value, false);

  draft.forceModelPrefix = false;
  draft.wsAuth = true;
  draft.debug = true;
  assert.deepEqual(state.settingsPatch.value, {
    debug: true,
    forceModelPrefix: false,
    wsAuth: true,
  });
  await state.saveSettings();
  assert.deepEqual(writes, [
    { debug: true, forceModelPrefix: false, wsAuth: true },
  ]);
  assert.equal(state.settings.value.debug, true);
  assert.equal(state.settings.value.forceModelPrefix, false);
  assert.equal(state.settings.value.wsAuth, true);
});

test("clearing a limit leaves it untouched while a new number is sent", async (t) => {
  const { state } = section(t);
  await open(state);
  const draft = state.settingsDraft.value;
  draft.logsMaxTotalSizeMb = undefined;
  assert.equal(state.settingsDirty.value, false);
  draft.errorLogsMaxFiles = 25;
  assert.deepEqual(state.settingsPatch.value, { errorLogsMaxFiles: 25 });
});

test("switching accounts drops the loaded settings and the open state", async (t) => {
  const props = reactive({ provider: provider(), revision: 1 });
  const { state } = section(t, {}, props);
  await open(state);
  assert.ok(state.settings.value);
  assert.equal(state.settingsOpen.value, true);

  props.provider = provider("other-account");
  await settle();
  assert.equal(state.settings.value, null);
  assert.equal(state.settingsDraft.value, null);
  assert.equal(state.settingsOpen.value, false);
});
