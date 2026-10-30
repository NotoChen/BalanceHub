import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, nextTick, reactive, ref } from "vue";
import { createPinia, setActivePinia, defineStore } from "pinia";
import { loadSource } from "./helpers/load-source.ts";
import { withTimeout } from "../src/utils/promise-timeout.ts";
import {
  managedModelCount,
  quotaNumber,
  quotaPercent,
  quotaResetLabel,
} from "../src/utils/oauth-display.ts";
import { providerBoardSection } from "../src/utils/provider-board-sections.ts";
import { quotaLevel } from "../src/utils/quota-level.ts";
import * as excludedModelRules from "../src/utils/cpa-excluded-models.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import type {
  CpaAccountSummary,
  CpaErrorLog,
  CpaLogSettings,
  CpaLogTail,
  OAuthAccountDetails,
  OAuthImportPreview,
  OAuthLoginTask,
} from "../src/stores/oauth-types.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
async function settle() {
  for (let i = 0; i < 16; i++) await Promise.resolve();
  await nextTick();
}

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
  const events: unknown[][] = [];
  const app = renderer.createApp({
    setup() {
      state = component.setup(props, {
        expose() {},
        emit: (...args: unknown[]) => events.push(args),
      });
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
  return { state, events, dispose };
}

function provider(id = "oauth-fixture"): Provider {
  return {
    identity: { id, name: id, remark: "" },
    auth: {
      credentialRevision: 1,
      mode: "accessToken",
      oauth: {
        vendor: "codex",
        email: "person@example.test",
        scopes: [],
        refreshPending: false,
        refreshError: null,
      },
    },
    capabilities: { subscription: null, availableModels: [] },
    automation: { refreshInterval: 0 },
    runtime: { enabled: true },
    actions: { oauthAccount: true, cpaGateway: false },
  } as unknown as Provider;
}
const messages: string[] = [];
function gatewayPanel(
  t: TestContext,
  props: Record<string, unknown>,
  bindings: Record<string, unknown>,
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaGatewayPanel.vue",
    {
      ...shared,
      "../../stores/providers": {
        useProviderStore: () => ({ refreshByIds: async () => null }),
      },
      "../ContentEditor.vue": {},
      "../FileEditorModal.vue": {},
      "./CpaSchedulingSection.vue": {},
      "./CpaModelRulesSection.vue": {},
      "./CpaClientKeysSection.vue": {},
      "./CpaPluginsDialog.vue": {},
      "./CpaLoginDialog.vue": {},
      "./CpaAccountQuota.vue": {},
      "./CpaRequestLogDialog.vue": {},
      "./CpaErrorLogDialog.vue": {},
      "./CpaUsageDialog.vue": {},
      ...bindings,
    },
  );
  return mount(t, component, props);
}
const shared = {
  "@lucide/vue": {},
  "@arco-design/web-vue": {
    Message: {
      error: (v: unknown) => messages.push(String(v)),
      info() {},
      success() {},
    },
  },
  "../../utils/quota-level": { quotaLevel },
  "../../utils/oauth-display": {
    oauthVendors: [],
    oauthVendorLabel: (v: string) => v,
    quotaNumber,
    managedModelCount,
    accountTimestamp: () => "",
  },
  "../../composables/provider-credential-dialogs": {
    confirmAction: async () => true,
  },
  /// Line parsing has its own suite; these tests only need the raw text back
  /// so a filter and the level list still have something to work with.
  "../../utils/cpa-log": {
    parseCpaLogLine: (raw: string) => ({
      raw,
      timestamp: "",
      requestId: "",
      level: "info",
      source: "",
      message: raw,
    }),
    cpaLogLevels: () => ["all"],
  },
};

function exportPanel(
  t: TestContext,
  confirm: () => Promise<boolean>,
  send: (id: string, gateway: string) => Promise<unknown>,
) {
  const updated: Provider[] = [];
  const refreshed: string[][] = [];
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthExportPanel.vue",
    {
      ...shared,
      "@tauri-apps/plugin-dialog": {},
      "../../composables/provider-credential-dialogs": {
        confirmAction: confirm,
      },
      "../../api/oauth-accounts": { sendOAuthToCpa: send },
      "../../stores/providers": {
        useProviderStore: () => ({
          providers: [
            {
              ...provider("gateway"),
              displayLabel: "演示 CPA",
              actions: { cpaGateway: true },
            },
          ],
          upsertProvider: (value: Provider) => updated.push(value),
          refreshByIds: async (ids: string[]) => refreshed.push(ids),
        }),
      },
    },
  );
  const mounted = mount(t, component, {
    account: {
      provider: provider(),
      exportOptions: [{ format: "cpa", available: true }],
    },
  });
  mounted.state.gatewayId.value = "gateway";
  return { ...mounted, updated, refreshed };
}

test("CPA transfer confirms and uploads only once even when clicked again while pending", async (t) => {
  const confirmation = deferred<boolean>();
  const response = deferred<{ provider: Provider; instructions: string }>();
  const sent: string[][] = [];
  let confirmations = 0;
  const { state, dispose, events, updated, refreshed } = exportPanel(
    t,
    () => {
      confirmations++;
      return confirmation.promise;
    },
    (id, gateway) => {
      sent.push([id, gateway]);
      return response.promise;
    },
  );
  const transferring = state.transfer();
  await state.transfer();
  assert.equal(confirmations, 1);
  assert.deepEqual(sent, []);
  confirmation.resolve(true);
  await settle();
  await state.transfer();
  assert.deepEqual(sent, [["oauth-fixture", "gateway"]]);
  dispose();
  response.resolve({ provider: provider(), instructions: "已交接" });
  await transferring;
  assert.equal(state.busy.value, false);
  assert.equal(
    updated.length,
    1,
    "the completed task still updates the global store",
  );
  assert.deepEqual(refreshed, [["gateway"]]);
  assert.deepEqual(events, [], "a closed panel receives no late result");
});

test("cancelling or closing a CPA confirmation never uploads credentials", async (t) => {
  for (const outcome of ["cancel", "close", "failure"]) {
    await t.test(outcome, async (t) => {
      const confirmation = deferred<boolean>();
      let sent = 0;
      const { state, dispose } = exportPanel(
        t,
        () => confirmation.promise,
        async () => {
          sent++;
        },
      );
      const transferring = state.transfer();
      if (outcome === "close") dispose();
      if (outcome === "failure")
        confirmation.reject(new Error("fixture confirmation failure"));
      else confirmation.resolve(outcome === "close");
      await transferring;
      assert.equal(sent, 0);
      assert.equal(state.busy.value, false);
      if (outcome === "failure")
        assert.match(state.error.value, /fixture confirmation failure/);
    });
  }
});

test("CPA remote toggle captures its credential revision and releases state after failure", async (t) => {
  const confirmation = deferred<boolean>();
  const response = deferred<void>();
  const props = reactive({ provider: provider("gateway") });
  const sent: unknown[][] = [];
  let confirmations = 0;
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaGatewayPanel.vue",
    {
      ...shared,
      "../../composables/provider-credential-dialogs": {
        confirmAction: () => {
          confirmations++;
          return confirmation.promise;
        },
      },
      "../../api/oauth-accounts": {
        setCpaDisabled: (...args: unknown[]) => {
          sent.push(args);
          return response.promise;
        },
      },
      "../../stores/providers": {
        useProviderStore: () => ({ refreshByIds: async () => null }),
      },
      "../ContentEditor.vue": {},
      "../FileEditorModal.vue": {},
      "./CpaSchedulingSection.vue": {},
      "./CpaModelRulesSection.vue": {},
      "./CpaClientKeysSection.vue": {},
      "./CpaPluginsDialog.vue": {},
      "./CpaLoginDialog.vue": {},
      "./CpaAccountQuota.vue": {},
      "./CpaRequestLogDialog.vue": {},
      "./CpaErrorLogDialog.vue": {},
      "./CpaUsageDialog.vue": {},
    },
  );
  const { state } = mount(t, component, props);
  const account = {
    name: "fixture.json",
    authIndex: "remote-index",
    disabled: false,
  } as CpaAccountSummary;
  const toggling = state.toggle(account);
  await state.toggle(account);
  assert.equal(confirmations, 1);
  props.provider.auth.credentialRevision = 2;
  confirmation.resolve(true);
  await settle();
  assert.deepEqual(sent, [
    ["gateway", "fixture.json", "remote-index", true, 1],
  ]);
  response.reject(new Error("fixture stale credential"));
  await toggling;
  assert.equal(state.pendingName.value, null);
  assert.match(state.error.value, /fixture stale credential/);
});

test("renewal confirmation cannot enable a replaced authorization", async (t) => {
  const confirmation = deferred<boolean>();
  const props = reactive({ provider: provider() });
  let writes = 0;
  let confirmations = 0;
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthAccountPanel.vue",
    {
      ...shared,
      "../../composables/provider-credential-dialogs": {
        confirmAction: () => {
          confirmations++;
          return confirmation.promise;
        },
      },
      "../../api/oauth-accounts": {
        getOAuthAccount: async () => ({ provider: props.provider }),
        setOAuthRenewal: async () => {
          writes++;
          return props.provider;
        },
      },
      "../../stores/providers": {
        useProviderStore: () => ({ upsertProvider() {} }),
      },
      "./AccountQuotaBar.vue": {},
      "../AgentCliIcon.vue": {},
      "./OAuthExportPanel.vue": {},
      "./OAuthCredentialEditor.vue": {},
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  const enabling = state.renewalChanged(true);
  await state.renewalChanged(true);
  assert.equal(confirmations, 1);
  props.provider.auth.credentialRevision = 2;
  confirmation.resolve(true);
  await enabling;
  assert.equal(writes, 0);
  assert.equal(state.busy.value, false);
});

function oauthLauncher(store: Record<string, any>) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthWorkspace.vue",
    {
      ...shared,
      "../../stores/oauth-accounts": { useOAuthAccountStore: () => store },
      "../../stores/providers": { useProviderStore: () => ({ providers: [] }) },
      "../../api/oauth-accounts": {
        startOAuthLogin: () => store.__started.promise,
      },
      "../AgentCliIcon.vue": {},
    },
  );
  return { default: component };
}
function launcherStore() {
  const started = deferred<OAuthLoginTask>();
  return {
    started,
    store: reactive<Record<string, any>>({
      visible: true,
      view: "add",
      providerId: null,
      taskId: null,
      tasks: {},
      close() {
        this.visible = false;
      },
      accept(task: OAuthLoginTask) {
        this.tasks[task.id] = task;
      },
      openTask(id: string) {
        this.taskId = id;
        this.view = "task";
        this.visible = true;
      },
      __started: started,
    }),
  };
}

test("authorization turns its own launcher into the live progress panel", async (t) => {
  const { store, started } = launcherStore();
  const { default: component } = oauthLauncher(store);
  const { state } = mount(t, component);
  const starting = state.start("codex");
  assert.equal(
    store.visible,
    true,
    "the launcher stays open instead of handing off to the task center",
  );
  assert.equal(store.view, "add");
  assert.equal(state.starting.value, "codex", "the clicked vendor shows progress");
  // A second click while the first start is pending never opens a second flow.
  await state.start("claude");
  started.resolve({ id: "task-fixture", vendor: "codex" } as OAuthLoginTask);
  await starting;
  assert.equal(store.view, "task");
  assert.equal(store.taskId, "task-fixture");
  assert.equal(state.starting.value, null);
});

test("a late authorization never replaces a panel opened in the meantime", async (t) => {
  const { store, started } = launcherStore();
  const { default: component } = oauthLauncher(store);
  const { state } = mount(t, component);
  const starting = state.start("codex");
  store.view = "import";
  started.resolve({ id: "task-fixture", vendor: "codex" } as OAuthLoginTask);
  await starting;
  assert.equal(store.view, "import", "a newer panel is never replaced");
  assert.equal(store.taskId, null);
  assert.equal(
    store.tasks["task-fixture"].id,
    "task-fixture",
    "the task is still recorded for the task center",
  );
});

test("an authorization that finishes after the launcher closed never reopens it", async (t) => {
  const { store, started } = launcherStore();
  const { default: component } = oauthLauncher(store);
  const { state } = mount(t, component);
  const starting = state.start("codex");
  store.visible = false;
  started.resolve({ id: "task-fixture", vendor: "codex" } as OAuthLoginTask);
  await starting;
  assert.equal(store.visible, false, "closing the launcher is final");
  assert.equal(state.starting.value, null, "busy state is released");
});

test("model counts distinguish an unread list from a successful empty list", () => {
  const account = provider();
  assert.equal(managedModelCount(account), null);
  account.capabilities.availableModels = ["fixture-model"];
  assert.equal(managedModelCount(account), 1);
  account.capabilities.availableModels = [];
  account.capabilities.availableModelsState = {
    updatedAt: "1900000000000",
    error: null,
    scope: "account",
    fallbackReason: null,
  };
  assert.equal(managedModelCount(account), 0);
});

test("account load ignores obsolete results and refresh timeout releases its busy state", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const first = deferred<OAuthAccountDetails>();
  const second = deferred<OAuthAccountDetails>();
  const props = reactive({ provider: provider("first") });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthAccountPanel.vue",
    {
      ...shared,
      "../../api/oauth-accounts": {
        getOAuthAccount: (id: string) =>
          id === "first" ? first.promise : second.promise,
      },
      "../../stores/providers": {
        useProviderStore: () => ({
          refreshByIds: () =>
            withTimeout(new Promise(() => {}), 50, "fixture timeout"),
        }),
      },
      "./AccountQuotaBar.vue": {},
      "../AgentCliIcon.vue": {},
      "./OAuthExportPanel.vue": {},
      "./OAuthCredentialEditor.vue": {},
    },
  );
  const { state } = mount(t, component, props);
  props.provider = provider("second");
  await nextTick();
  second.resolve({ provider: props.provider } as OAuthAccountDetails);
  await settle();
  first.resolve({ provider: provider("first") } as OAuthAccountDetails);
  await settle();
  assert.equal(state.details.value.provider.identity.id, "second");
  assert.equal(state.loading.value, false);
  const refreshing = state.refresh();
  assert.equal(state.busy.value, true);
  t.mock.timers.tick(51);
  await refreshing;
  assert.equal(state.busy.value, false);
  assert.match(state.error.value, /fixture timeout/);
});

test("closing an import drops a late secret preview instead of attaching it to a new panel", async (t) => {
  const pending = deferred<OAuthImportPreview>();
  const discarded: string[] = [];
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthImportPanel.vue",
    {
      ...shared,
      "@tauri-apps/plugin-dialog": { open: async () => [] },
      "../../api/oauth-accounts": {
        previewOAuthImport: () => pending.promise,
        discardOAuthImport: async (token: string) => {
          discarded.push(token);
        },
      },
      "../../stores/providers": { useProviderStore: () => ({}) },
      "../AgentCliIcon.vue": {},
    },
  );
  const { state, dispose, events } = mount(t, component, {});
  state.text.value = "{}";
  const reading = state.readFiles(true);
  dispose();
  pending.resolve({ token: "late-preview", entries: [], errors: [] });
  await reading;
  assert.deepEqual(discarded, ["late-preview"]);
  assert.equal(state.preview.value, null);
  assert.deepEqual(events, []);
});

test("batch import schedules a refresh for every imported account", async (t) => {
  const refreshed: string[][] = [];
  const imported = [provider("first"), provider("second")];
  const preview = {
    token: "preview",
    entries: [
      { id: "one", duplicateId: null },
      { id: "two", duplicateId: null },
    ],
    errors: [],
  };
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthImportPanel.vue",
    {
      ...shared,
      "@tauri-apps/plugin-dialog": {},
      "../../api/oauth-accounts": {
        applyOAuthImport: async () => imported,
        discardOAuthImport: async () => {},
      },
      "../../stores/providers": {
        useProviderStore: () => ({
          upsertProviders() {},
          refreshByIds: async (ids: string[]) => {
            refreshed.push(ids);
          },
        }),
      },
      "../AgentCliIcon.vue": {},
    },
  );
  const { state } = mount(t, component, { initialPreview: preview });
  await state.importSelected();
  assert.equal(state.busy.value, false);
  assert.deepEqual(refreshed, [["first", "second"]]);
});

test("out-of-order task events cannot regress a completed login", () => {
  setActivePinia(createPinia());
  const { useOAuthAccountStore } = loadSource<{
    useOAuthAccountStore: () => any;
  }>("stores/oauth-accounts.ts", { pinia: { defineStore } });
  const store = useOAuthAccountStore();
  store.accept({ id: "task", phase: "completed", revision: 3 });
  store.accept({ id: "task", phase: "waiting", revision: 1 });
  assert.equal(store.tasks.task.phase, "completed");
});

test("the account card click still respects drag suppression", async (t) => {
  const opened: string[] = [];
  const suppressed = ref(true);
  const { useProviderWorkspaceController } = loadSource<{
    useProviderWorkspaceController: (options: any) => any;
  }>("composables/useProviderWorkspaceController.ts", {
    "@arco-design/web-vue": shared["@arco-design/web-vue"],
    "../utils/provider-actions": { providerNeedsCheckIn: () => false },
    "../utils/provider-liveness": { providerLivenessEnabled: () => false },
    "../utils/provider-board-sections": { providerBoardSection },
    "../stores/oauth-accounts": {
      useOAuthAccountStore: () => ({
        openAccount: (id: string) => opened.push(id),
      }),
    },
    "./useProviderCardTone": {
      useProviderCardTone: () => ({ providerIntermediateLabel: () => "" }),
    },
    "./useCardDragSort": {
      useCardDragSort: () => ({
        state: {},
        clickSuppressed: suppressed,
        orderedGroups: ref(new Map()),
        draggedItem: ref(null),
        overId: ref(null),
      }),
    },
  });
  const { state } = mount(t, {
    setup: () =>
      useProviderWorkspaceController({ providers: ref([]), settings: ref({}) }),
  });
  await state.handleProviderCardClick(provider());
  assert.deepEqual(opened, []);
  await state.handleProviderCardClick(provider());
  assert.deepEqual(opened, ["oauth-fixture"]);
});

test("quota labels use at most two decimals and grids stay separate", () => {
  assert.equal(quotaNumber(12.3456789), "12.35");
  assert.equal(quotaNumber(null), "—");
  assert.equal(quotaNumber(Number.NaN), "—");
  assert.equal(quotaPercent(23.456), "23.5");
  assert.equal(quotaPercent(47), "47");
  assert.equal(quotaPercent(null), "—");
  assert.equal(quotaResetLabel(1, 60_000), "已到重置时间");
  const oauth = provider();
  const gateway = {
    ...oauth,
    actions: { ...oauth.actions, oauthAccount: false, cpaGateway: true },
  };
  assert.equal(providerBoardSection(oauth, false), "oauth");
  assert.equal(providerBoardSection(gateway, false), "cpa");
  assert.equal(providerBoardSection(gateway, true), "liveness");
});

test("CPA field edits capture the connection revision and release state after failure", async (t) => {
  const response = deferred<void>();
  const props = reactive({ provider: provider("gateway") });
  const sent: unknown[][] = [];
  let reloads = 0;
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaGatewayPanel.vue",
    {
      ...shared,
      "../../api/oauth-accounts": {
        updateCpaFields: (...args: unknown[]) => {
          sent.push(args);
          return response.promise;
        },
      },
      "../../stores/providers": {
        useProviderStore: () => ({
          refreshByIds: async () => {
            reloads++;
            return null;
          },
        }),
      },
      "../ContentEditor.vue": {},
      "../FileEditorModal.vue": {},
      "./CpaSchedulingSection.vue": {},
      "./CpaModelRulesSection.vue": {},
      "./CpaClientKeysSection.vue": {},
      "./CpaPluginsDialog.vue": {},
      "./CpaLoginDialog.vue": {},
      "./CpaAccountQuota.vue": {},
      "./CpaRequestLogDialog.vue": {},
      "./CpaErrorLogDialog.vue": {},
      "./CpaUsageDialog.vue": {},
    },
  );
  const { state } = mount(t, component, props);
  const account = {
    name: "fixture.json",
    authIndex: "remote-index",
    disabled: false,
    priority: 3,
    weight: 7,
    requestRetry: null,
    note: "",
    excludedModels: ["fixture-excluded"],
  } as CpaAccountSummary;
  state.editFields(account);
  assert.equal(state.editorVisible.value, true);
  state.editor.value.note = "fixture note";
  const saving = state.saveFields();
  await state.saveFields();
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].slice(0, 3), [
    "gateway",
    "fixture.json",
    "remote-index",
  ]);
  assert.equal(sent[0][4], 1, "the revision was captured before the request");
  assert.deepEqual(sent[0][3], {
    priority: 3,
    weight: 7,
    requestRetry: null,
    note: "fixture note",
    excludedModels: ["fixture-excluded"],
  });
  props.provider.auth.credentialRevision = 2;
  response.reject(new Error("fixture field failure"));
  await saving;
  assert.equal(state.editorSaving.value, false);
  assert.match(state.editorError.value, /fixture field failure/);
  assert.equal(reloads, 0);
});

test("CPA removal requires confirmation and releases the row lock after failure", async (t) => {
  const confirmations = [deferred<boolean>(), deferred<boolean>()];
  let confirmationCalls = 0;
  const response = deferred<void>();
  const props = reactive({ provider: provider("gateway") });
  const removed: unknown[][] = [];
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaGatewayPanel.vue",
    {
      ...shared,
      "../../composables/provider-credential-dialogs": {
        confirmAction: () => confirmations[confirmationCalls++].promise,
      },
      "../../api/oauth-accounts": {
        deleteCpaAccount: (...args: unknown[]) => {
          removed.push(args);
          return response.promise;
        },
      },
      "../../stores/providers": {
        useProviderStore: () => ({ refreshByIds: async () => null }),
      },
      "../ContentEditor.vue": {},
      "../FileEditorModal.vue": {},
      "./CpaSchedulingSection.vue": {},
      "./CpaModelRulesSection.vue": {},
      "./CpaClientKeysSection.vue": {},
      "./CpaPluginsDialog.vue": {},
      "./CpaLoginDialog.vue": {},
      "./CpaAccountQuota.vue": {},
      "./CpaRequestLogDialog.vue": {},
      "./CpaErrorLogDialog.vue": {},
      "./CpaUsageDialog.vue": {},
    },
  );
  const { state } = mount(t, component, props);
  const account = {
    name: "fixture.json",
    authIndex: "remote-index",
    disabled: false,
  } as CpaAccountSummary;
  const deleting = state.removeRemote(account);
  await state.removeRemote(account);
  assert.equal(confirmationCalls, 1, "a pending row ignores a second action");
  confirmations[0].resolve(false);
  await deleting;
  assert.equal(removed.length, 0, "cancelling never deletes remotely");
  assert.equal(state.pendingName.value, null);

  const second = state.removeRemote(account);
  confirmations[1].resolve(true);
  await settle();
  assert.equal(removed.length, 1);
  assert.deepEqual(removed[0], [
    "gateway",
    "fixture.json",
    "remote-index",
    1,
  ]);
  response.reject(new Error("fixture delete failure"));
  await second;
  assert.equal(state.pendingName.value, null);
  assert.match(state.error.value, /fixture delete failure/);
});

test("credential editor ignores a replaced account and releases saving after failure", async (t) => {
  const first = deferred<string>();
  const second = deferred<string>();
  const response = deferred<Provider>();
  const props = reactive({
    provider: provider("first"),
    visible: true,
  });
  const saved: unknown[][] = [];
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/OAuthCredentialEditor.vue",
    {
      ...shared,
      "../../api/oauth-accounts": {
        getOAuthCredential: (id: string) =>
          id === "first" ? first.promise : second.promise,
        replaceOAuthCredential: (...args: unknown[]) => {
          saved.push(args);
          return response.promise;
        },
      },
      "../ContentEditor.vue": {},
      "../FileEditorModal.vue": {},
      "./CpaSchedulingSection.vue": {},
      "./CpaModelRulesSection.vue": {},
      "./CpaClientKeysSection.vue": {},
      "./CpaPluginsDialog.vue": {},
      "./CpaLoginDialog.vue": {},
      "./CpaAccountQuota.vue": {},
      "./CpaRequestLogDialog.vue": {},
      "./CpaErrorLogDialog.vue": {},
      "./CpaUsageDialog.vue": {},
    },
  );
  const { state } = mount(t, component, props);
  props.provider = provider("second");
  props.visible = false;
  await nextTick();
  props.visible = true;
  await nextTick();
  second.resolve('{"format":"balancehub-oauth"}');
  await settle();
  first.resolve('{"stale":true}');
  await settle();
  assert.equal(state.original.value, '{"format":"balancehub-oauth"}');
  assert.equal(state.loading.value, false);

  state.draft.value = '{"format":"balancehub-oauth","edited":true}';
  const saving = state.save();
  await settle();
  assert.equal(state.saving.value, true);
  assert.deepEqual(saved[0], [
    "second",
    '{"format":"balancehub-oauth","edited":true}',
    1,
  ]);
  response.reject(new Error("fixture credential failure"));
  await saving;
  assert.equal(state.saving.value, false);
  assert.match(state.error.value, /fixture credential failure/);
});

function clientKeysPanel(
  t: TestContext,
  api: Record<string, (...args: any[]) => Promise<unknown>>,
  props: any = { provider: provider("gateway"), revision: 3 },
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaClientKeysSection.vue",
    {
      ...shared,
      "../../composables/useClipboard": { copyText: async () => {} },
      "../../api/oauth-accounts": api,
    },
  );
  return mount(t, component, props);
}

test("CPA client keys load once per open and drop results from a replaced gateway", async (t) => {
  const first = deferred<{ keys: string[] }>();
  const keys: string[][] = [];
  const props = reactive({ provider: provider("gateway"), revision: 3 });
  const { state } = clientKeysPanel(
    t,
    {
      listCpaClientKeys: (id: string) => {
        keys.push([id]);
        return first.promise;
      },
    },
    props,
  );
  state.toggleKeys();
  assert.equal(state.keysOpen.value, true);
  assert.equal(state.keysLoading.value, true);
  // Re-opening never starts a second read for the same gateway.
  state.toggleKeys();
  state.toggleKeys();
  assert.equal(keys.length, 1);
  props.provider = provider("other-gateway");
  await settle();
  first.resolve({ keys: ["sk-late"] });
  await settle();
  assert.equal(
    state.clientKeys.value,
    null,
    "a stale list never lands on the replaced card",
  );
  assert.equal(state.keysLoading.value, false);
});

test("CPA client key adds reveal the minted value and release state after failure", async (t) => {
  const saved = deferred<{ list: { keys: string[] }; value: string }>();
  const sent: unknown[][] = [];
  const { state } = clientKeysPanel(t, {
    listCpaClientKeys: async () => ({ keys: ["sk-existing"] }),
    addCpaClientKey: (id: string, value: string, revision: number) => {
      sent.push([id, value, revision]);
      return saved.promise;
    },
  });
  state.toggleKeys();
  await settle();
  state.keyDraft.value = "  ";
  const adding = state.addKey();
  await state.addKey();
  assert.deepEqual(sent, [["gateway", "", 3]]);
  saved.resolve({ list: { keys: ["sk-existing", "sk-minted"] }, value: "sk-minted" });
  await adding;
  assert.deepEqual(state.clientKeys.value, ["sk-existing", "sk-minted"]);
  assert.equal(state.revealedKey.value, "sk-minted");
  assert.equal(state.keyDraft.value, "");
  assert.equal(state.keysSaving.value, false);

  const failure = deferred<never>();
  const failing = clientKeysPanel(t, {
    listCpaClientKeys: async () => ({ keys: [] }),
    addCpaClientKey: () => failure.promise,
  });
  failing.state.toggleKeys();
  await settle();
  const pending = failing.state.addKey();
  failure.reject(new Error("fixture add failure"));
  await pending;
  assert.match(failing.state.keysError.value, /fixture add failure/);
  assert.equal(failing.state.keysSaving.value, false);
});

test("CPA client key removal asks first and keeps the list untouched on cancel", async (t) => {
  const removed: unknown[][] = [];
  let confirmations = 0;
  let allow = false;
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaClientKeysSection.vue",
    {
      ...shared,
      "../../composables/useClipboard": { copyText: async () => {} },
      "../../composables/provider-credential-dialogs": {
        confirmAction: async () => {
          confirmations++;
          return allow;
        },
      },
      "../../api/oauth-accounts": {
        listCpaClientKeys: async () => ({ keys: ["sk-one", "sk-two"] }),
        removeCpaClientKey: (id: string, value: string, revision: number) => {
          removed.push([id, value, revision]);
          return Promise.resolve({ keys: ["sk-two"] });
        },
      },
    },
  );
  const { state } = mount(t, component, {
    provider: provider("gateway"),
    revision: 4,
  });
  state.toggleKeys();
  await settle();
  await state.removeKey("sk-one");
  assert.equal(confirmations, 1);
  assert.deepEqual(removed, []);
  assert.deepEqual(state.clientKeys.value, ["sk-one", "sk-two"]);
  allow = true;
  await state.removeKey("sk-one");
  assert.deepEqual(removed, [["gateway", "sk-one", 4]]);
  assert.deepEqual(state.clientKeys.value, ["sk-two"]);
});

function modelRulesPanel(
  t: TestContext,
  api: Record<string, (...args: any[]) => Promise<unknown>>,
  props: any = { provider: provider("gateway"), revision: 3 },
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaModelRulesSection.vue",
    {
      ...shared,
      "../ContentEditor.vue": {},
      "../../api/oauth-accounts": api,
      "../../utils/cpa-excluded-models": excludedModelRules,
    },
  );
  return mount(t, component, props);
}

test("CPA model rules load once per open and drop a replaced gateway's answer", async (t) => {
  const first = deferred<{ excludedModels: string; modelAliases: string }>();
  const reads: string[][] = [];
  const props = reactive({ provider: provider("gateway"), revision: 3 });
  const { state } = modelRulesPanel(
    t,
    {
      getCpaModelRules: (id: string) => {
        reads.push([id]);
        return first.promise;
      },
    },
    props,
  );
  state.toggle();
  assert.equal(state.open.value, true);
  assert.equal(state.loading.value, true);
  state.toggle();
  state.toggle();
  assert.equal(reads.length, 1, "re-opening never starts a second read");
  props.provider = provider("other-gateway");
  await settle();
  first.resolve({
    excludedModels: '{"claude":["old-model"]}',
    modelAliases: "{}",
  });
  await settle();
  assert.equal(state.rules.value, null, "a stale read never lands");
  assert.equal(state.loading.value, false);
});

test("CPA model rule edits write one map and adopt the confirmed rules", async (t) => {
  const saved = deferred<{ excludedModels: string; modelAliases: string }>();
  const writes: unknown[][] = [];
  const { state } = modelRulesPanel(t, {
    getCpaModelRules: async () => ({
      excludedModels: '{\n  "claude": [\n    "opus"\n  ]\n}',
      modelAliases: "{}",
    }),
    updateCpaModelRules: (
      id: string,
      kind: string,
      content: string,
      revision: number,
    ) => {
      writes.push([id, kind, content, revision]);
      return saved.promise;
    },
  });
  state.toggle();
  await settle();
  state.openEditor("modelAliases");
  assert.equal(state.editing.value, "modelAliases");
  assert.equal(state.draft.value, "{}");
  state.draft.value = '{"Codex":[{"name":"gpt-5","alias":"fast"}]}';
  assert.equal(state.dirty(), true);
  const saving = state.save();
  // A second click while the write is pending never duplicates it.
  await state.save();
  assert.deepEqual(writes, [
    [
      "gateway",
      "modelAliases",
      '{"Codex":[{"name":"gpt-5","alias":"fast"}]}',
      3,
    ],
  ]);
  saved.resolve({
    excludedModels: '{\n  "claude": [\n    "opus"\n  ]\n}',
    modelAliases: '{\n  "codex": [\n    {\n      "name": "gpt-5",\n      "alias": "fast"\n    }\n  ]\n}',
  });
  await saving;
  assert.equal(state.dirty(), false, "the confirmed text becomes the new draft");
  assert.equal(state.editing.value, "modelAliases", "the editor stays open");
  assert.deepEqual(
    state.aliasRows.value,
    [{ channel: "codex", count: 1 }],
    "the normalized gateway answer is what the section shows",
  );
  assert.equal(state.saving.value, false);
});

test("a failed model rule write keeps the draft and releases the editor", async (t) => {
  const failure = deferred<never>();
  const { state } = modelRulesPanel(t, {
    getCpaModelRules: async () => ({
      excludedModels: "{}",
      modelAliases: "{}",
    }),
    getCpaModelDefinitions: async () => [
      { id: "opus", displayName: "Opus", modelType: "claude", ownedBy: "", contextLength: null },
    ],
    updateCpaModelRules: () => failure.promise,
  });
  state.toggle();
  await settle();
  state.openEditor("excludedModels");
  await settle();
  state.setCustom("opus");
  const saving = state.save();
  failure.reject(new Error("fixture rule failure"));
  await saving;
  assert.match(state.editorError.value, /fixture rule failure/);
  assert.deepEqual(state.drafts.value.claude, ["opus"], "the draft survives");
  assert.equal(state.saving.value, false);
  assert.equal(state.editing.value, "excludedModels");
});

test("a model rule write that lands after the editor closed only refreshes the section", async (t) => {
  const saved = deferred<{ excludedModels: string; modelAliases: string }>();
  const { state } = modelRulesPanel(t, {
    getCpaModelRules: async () => ({
      excludedModels: "{}",
      modelAliases: "{}",
    }),
    getCpaModelDefinitions: async () => [],
    updateCpaModelRules: () => saved.promise,
  });
  state.toggle();
  await settle();
  state.openEditor("excludedModels");
  state.setCustom("opus");
  const saving = state.save();
  state.closeEditor();
  saved.resolve({
    excludedModels: '{\n  "claude": [\n    "opus"\n  ]\n}',
    modelAliases: "{}",
  });
  await saving;
  assert.equal(state.editing.value, null, "closing the editor is never undone");
  assert.deepEqual(state.drafts.value, {}, "a closed editor keeps no draft");
  assert.deepEqual(
    state.excludedRows.value,
    [{ channel: "claude", count: 1 }],
    "the section still shows what the gateway confirmed",
  );
  assert.equal(state.saving.value, false);
});

test("a background card refresh collapses model rules but keeps the open draft", async (t) => {
  const props = reactive({ provider: provider("gateway"), revision: 1 });
  const { state } = modelRulesPanel(
    t,
    {
      getCpaModelRules: async () => ({
        excludedModels: "{}",
        modelAliases: '{\n  "codex": []\n}',
      }),
    },
    props,
  );
  state.toggle();
  await settle();
  state.openEditor("modelAliases");
  state.draft.value = '{"codex":[{"name":"gpt-5","alias":"fast"}]}';
  props.revision = 2;
  await settle();
  assert.equal(state.open.value, false, "the section follows the new revision");
  assert.equal(state.rules.value, null);
  assert.equal(
    state.draft.value,
    '{"codex":[{"name":"gpt-5","alias":"fast"}]}',
    "a refresh does not discard a half-written rule",
  );
});

test("the exclusion editor offers the channel catalog and writes checked models", async (t) => {
  const written: unknown[][] = [];
  const reads: unknown[][] = [];
  const { state } = modelRulesPanel(t, {
    getCpaModelRules: async () => ({
      excludedModels: '{"claude":["opus"]}',
      modelAliases: "{}",
    }),
    getCpaModelDefinitions: async (id: string, channel: string) => {
      reads.push([id, channel]);
      return [
        { id: "opus", displayName: "Opus", modelType: "claude", ownedBy: "anthropic", contextLength: 200000 },
        { id: "haiku", displayName: "Haiku", modelType: "claude", ownedBy: "anthropic", contextLength: 200000 },
      ];
    },
    updateCpaModelRules: (
      id: string,
      kind: string,
      content: string,
      revision: number,
    ) => {
      written.push([id, kind, content, revision]);
      return Promise.resolve({ excludedModels: content, modelAliases: "{}" });
    },
  });
  state.toggle();
  await settle();
  state.openEditor("excludedModels");
  await settle();
  assert.deepEqual(reads, [["gateway", "claude"]], "the catalog follows the channel");
  assert.equal(state.channel.value, "claude", "the only configured channel opens first");
  assert.equal(state.isChecked("opus"), true, "a saved rule ticks its catalog entry");
  assert.equal(state.isChecked("haiku"), false);
  assert.equal(state.customText.value, "", "catalog rules are not repeated as text");
  assert.deepEqual(state.stats.value, { total: 2, excluded: 1 });
  state.toggleModel("haiku", true);
  assert.equal(state.dirty(), true);
  await state.save();
  assert.deepEqual(written, [
    [
      "gateway",
      "excludedModels",
      '{\n  "claude": [\n    "opus",\n    "haiku"\n  ]\n}',
      3,
    ],
  ]);
});

test("a wildcard keeps a model excluded after its exact rule is unchecked", async (t) => {
  const { state } = modelRulesPanel(t, {
    getCpaModelRules: async () => ({
      excludedModels: '{"claude":["claude-3-opus","claude-3-*"]}',
      modelAliases: "{}",
    }),
    getCpaModelDefinitions: async () => [
      { id: "claude-3-opus", displayName: "", modelType: "claude", ownedBy: "", contextLength: null },
    ],
  });
  state.toggle();
  await settle();
  state.openEditor("excludedModels");
  await settle();
  assert.equal(state.isChecked("claude-3-opus"), true);
  assert.equal(
    state.wildcardFor("claude-3-opus"),
    "claude-3-*",
    "the row names the wildcard that also excludes it",
  );
  state.toggleModel("claude-3-opus", false);
  assert.equal(state.isChecked("claude-3-opus"), false);
  assert.equal(
    state.wildcardFor("claude-3-opus"),
    "claude-3-*",
    "unchecking the exact rule does not hide that the wildcard still applies",
  );
  assert.deepEqual(state.stats.value, { total: 1, excluded: 1 });
});

test("the exclusion editor keeps each channel's edits while switching channels", async (t) => {
  const written: unknown[][] = [];
  const { state } = modelRulesPanel(t, {
    getCpaModelRules: async () => ({
      excludedModels: '{"claude":[],"codex":[]}',
      modelAliases: "{}",
    }),
    getCpaModelDefinitions: async (_id: string, channel: string) =>
      channel === "codex"
        ? [{ id: "gpt-5", displayName: "", modelType: "codex", ownedBy: "", contextLength: null }]
        : [{ id: "opus", displayName: "", modelType: "claude", ownedBy: "", contextLength: null }],
    updateCpaModelRules: (
      id: string,
      kind: string,
      content: string,
      revision: number,
    ) => {
      written.push([id, kind, content, revision]);
      return Promise.resolve({ excludedModels: content, modelAliases: "{}" });
    },
  });
  state.toggle();
  await settle();
  state.openEditor("excludedModels");
  await settle();
  state.setCustom("claude-*");
  state.chooseChannel("codex");
  await settle();
  assert.equal(state.channel.value, "codex");
  assert.equal(state.customText.value, "", "the new channel starts from its own rules");
  state.toggleModel("gpt-5", true);
  state.chooseChannel("claude");
  await settle();
  assert.equal(state.customText.value, "claude-*", "the first channel kept its edit");
  await state.save();
  const body = JSON.parse(written[0][2] as string);
  assert.deepEqual(body, { claude: ["claude-*"], codex: ["gpt-5"] });
});

test("CPA plugins load on open, write the connection revision and release on failure", async (t) => {
  const toggle = deferred<void>();
  const toggles: unknown[][] = [];
  const props = reactive({
    visible: true,
    provider: provider("gateway"),
    revision: 5,
  });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaPluginsDialog.vue",
    {
      ...shared,
      "../ContentEditor.vue": {},
      "./CpaPluginStore.vue": {},
      "../../composables/provider-credential-dialogs": {
        confirmAction: async () => true,
      },
      "../../api/oauth-accounts": {
        listCpaPlugins: async () => ({
          enabled: true,
          plugins: [
            {
              id: "oauth-login",
              name: "OAuth Login",
              version: "1.0.0",
              enabled: false,
              effectiveEnabled: false,
              configured: false,
              registered: true,
              supportsQuota: false,
            },
          ],
        }),
        setCpaPluginEnabled: (
          id: string,
          pluginId: string,
          enabled: boolean,
          revision: number,
        ) => {
          toggles.push([id, pluginId, enabled, revision]);
          return toggle.promise;
        },
      },
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  const plugin = state.plugins.value.plugins[0];
  const pending = state.toggle(plugin, true);
  // A second click while the first write is pending never duplicates it.
  await state.toggle(plugin, false);
  assert.deepEqual(toggles, [["gateway", "oauth-login", true, 5]]);
  toggle.reject(new Error("fixture plugin failure"));
  await settle();
  await pending;
  assert.match(state.error.value, /fixture plugin failure/);
  assert.equal(state.toggling.value, null);
  assert.equal(plugin.enabled, false, "a failed write leaves the switch alone");
});

test("CPA plugin configuration closes without locking and ignores late reads", async (t) => {
  const read = deferred<{ content: string }>();
  const props = reactive({
    visible: true,
    provider: provider("gateway"),
    revision: 1,
  });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaPluginsDialog.vue",
    {
      ...shared,
      "../ContentEditor.vue": {},
      "./CpaPluginStore.vue": {},
      "../../api/oauth-accounts": {
        listCpaPlugins: async () => ({
          enabled: true,
          plugins: [
            {
              id: "oauth-login",
              name: "OAuth Login",
              version: "",
              enabled: true,
              effectiveEnabled: true,
              configured: true,
              registered: true,
              supportsQuota: false,
            },
          ],
        }),
        getCpaPluginConfig: () => read.promise,
      },
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  const opening = state.openConfig(state.plugins.value.plugins[0]);
  props.visible = false;
  await settle();
  assert.equal(state.editing.value, null);
  read.resolve({ content: "{}" });
  await opening;
  assert.equal(
    state.configText.value,
    "",
    "a closed dialog keeps no late configuration",
  );
  assert.equal(state.configLoading.value, false);
});

test("a background card refresh does not close an open plugin configuration", async (t) => {
  const props = reactive({
    visible: true,
    provider: provider("gateway"),
    revision: 1,
  });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaPluginsDialog.vue",
    {
      ...shared,
      "../ContentEditor.vue": {},
      "./CpaPluginStore.vue": {},
      "../../api/oauth-accounts": {
        listCpaPlugins: async () => ({
          enabled: true,
          plugins: [
            {
              id: "oauth-login",
              name: "OAuth Login",
              version: "",
              enabled: true,
              effectiveEnabled: true,
              configured: true,
              registered: true,
              supportsQuota: false,
            },
          ],
        }),
        getCpaPluginConfig: async () => ({ content: '{\n  "a": 1\n}' }),
      },
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  await state.openConfig(state.plugins.value.plugins[0]);
  assert.equal(state.editing.value?.id, "oauth-login");
  props.revision = 2;
  await settle();
  assert.equal(
    state.editing.value?.id,
    "oauth-login",
    "the draft survives a card refresh",
  );
  assert.equal(state.configText.value, '{\n  "a": 1\n}');
});

function loginDialog(
  t: TestContext,
  api: Record<string, (...args: any[]) => Promise<unknown>>,
  props: any = { visible: true, provider: provider("gateway") },
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaLoginDialog.vue",
    {
      ...shared,
      "../../composables/useClipboard": { copyText: async () => {} },
      "../../api/oauth-accounts": {
        listCpaLoginProviders: async () => [
          { id: "codex", label: "Codex", plugin: null },
        ],
        cancelCpaLogin: async () => {},
        ...api,
      },
    },
  );
  return mount(t, component, props);
}

test("CPA login lists the gateway's services and starts from the first", async (t) => {
  const reads: string[][] = [];
  const started: unknown[][] = [];
  const { state } = loginDialog(t, {
    listCpaLoginProviders: (id: string) => {
      reads.push([id]);
      return Promise.resolve([
        { id: "codex", label: "Codex", plugin: null },
        { id: "corp", label: "Corp SSO", plugin: "sso" },
      ]);
    },
    startCpaLogin: (id: string, service: string) => {
      started.push([id, service]);
      return Promise.resolve({
        state: "s1",
        url: "https://accounts.example.test/authorize",
        userCode: null,
        expiresIn: null,
        flow: "browser",
        apiVersion: "v8",
      });
    },
  });
  await settle();
  assert.deepEqual(reads, [["gateway"]]);
  assert.equal(state.selected.value, "codex", "the first service is offered");
  await state.start();
  await settle();
  assert.deepEqual(started, [["gateway", "codex"]]);
  assert.equal(state.isDevice.value, false);
  await state.start();
  assert.equal(started.length, 1, "a second start never opens a second session");
});

test("a started CPA login polls until the gateway reports completion", async (t) => {
  const statuses = [{ status: "waiting" }, { status: "completed" }];
  const { state, events } = loginDialog(t, {
    startCpaLogin: async () => ({
      state: "s1",
      url: "https://accounts.example.test/authorize",
      userCode: null,
      expiresIn: 600,
      flow: "browser",
      apiVersion: "v8",
    }),
    getCpaLoginStatus: async () => statuses.shift(),
  });
  await settle();
  await state.start();
  await settle();
  assert.equal(state.phase.value, "waiting");
  assert.deepEqual(events, []);
  await state.poll();
  await settle();
  assert.equal(state.phase.value, "completed");
  assert.deepEqual(events, [["completed"]]);
});

test("a failed CPA login keeps the gateway's reason and stops polling", async (t) => {
  let reads = 0;
  const { state } = loginDialog(t, {
    startCpaLogin: async () => ({
      state: "s1",
      url: "https://accounts.example.test/authorize",
      userCode: null,
      expiresIn: null,
      flow: "browser",
      apiVersion: "v8",
    }),
    getCpaLoginStatus: async () => {
      reads++;
      return { status: "failed", message: "授权会话已过期，请重新开始" };
    },
  });
  await settle();
  await state.start();
  await settle();
  assert.equal(state.phase.value, "failed");
  assert.equal(state.statusError.value, "授权会话已过期，请重新开始");
  await state.poll();
  assert.equal(reads, 1, "a finished session is no longer polled");
});

test("a status read that fails never ends the session on its own", async (t) => {
  let offline = true;
  const reads: string[][] = [];
  const { state } = loginDialog(t, {
    startCpaLogin: async () => ({
      state: "s1",
      url: "https://accounts.example.test/authorize",
      userCode: "ABCD-EFGH",
      expiresIn: null,
      flow: "device",
      apiVersion: "v8",
    }),
    getCpaLoginStatus: (id: string, value: string) => {
      reads.push([id, value]);
      return offline
        ? Promise.reject(new Error("fixture offline"))
        : Promise.resolve({ status: "waiting" });
    },
  });
  await settle();
  await state.start();
  await settle();
  assert.equal(state.phase.value, "waiting");
  assert.equal(state.statusError.value, "Error: fixture offline");
  assert.equal(state.isDevice.value, true);
  offline = false;
  await state.poll();
  assert.equal(state.statusError.value, "");
  assert.deepEqual(reads, [
    ["gateway", "s1"],
    ["gateway", "s1"],
  ]);
});

test("closing the dialog cancels the session the gateway was still holding", async (t) => {
  const cancelled: unknown[][] = [];
  const props = reactive({ visible: true, provider: provider("gateway") });
  const { state } = loginDialog(
    t,
    {
      startCpaLogin: async () => ({
        state: "s1",
        url: "https://accounts.example.test/authorize",
        userCode: null,
        expiresIn: null,
        flow: "browser",
        apiVersion: "v8",
      }),
      getCpaLoginStatus: async () => ({ status: "waiting" }),
      cancelCpaLogin: (id: string, value: string) => {
        cancelled.push([id, value]);
        return Promise.resolve();
      },
    },
    props,
  );
  await settle();
  await state.start();
  await settle();
  props.visible = false;
  await settle();
  assert.deepEqual(cancelled, [["gateway", "s1"]]);
  assert.equal(state.login.value, null);
});

test("a dialog closed while starting cancels the session it had just opened", async (t) => {
  const starting = deferred<any>();
  const cancelled: unknown[][] = [];
  const { state, dispose } = loginDialog(t, {
    startCpaLogin: () => starting.promise,
    cancelCpaLogin: (id: string, value: string) => {
      cancelled.push([id, value]);
      return Promise.resolve();
    },
  });
  await settle();
  const pending = state.start();
  dispose();
  starting.resolve({
    state: "s9",
    url: "https://accounts.example.test/authorize",
    userCode: null,
    expiresIn: null,
    flow: "browser",
    apiVersion: "v8",
  });
  await pending;
  assert.deepEqual(cancelled, [["gateway", "s9"]]);
});

test("the callback is submitted for the service the session started with", async (t) => {
  const submitted: unknown[][] = [];
  const { state } = loginDialog(t, {
    listCpaLoginProviders: async () => [
      { id: "codex", label: "Codex", plugin: null },
      { id: "claude", label: "Claude", plugin: null },
    ],
    startCpaLogin: async () => ({
      state: "s1",
      url: "https://accounts.example.test/authorize",
      userCode: null,
      expiresIn: null,
      flow: "browser",
      apiVersion: "v0",
    }),
    getCpaLoginStatus: async () => ({ status: "waiting" }),
    submitCpaLoginCallback: (...args: unknown[]) => {
      submitted.push(args);
      return Promise.resolve();
    },
  });
  await settle();
  await state.start();
  await settle();
  state.selected.value = "claude";
  state.callback.value = "http://localhost:1455/callback?code=xyz&state=s1";
  await state.submit();
  assert.deepEqual(submitted, [
    [
      "gateway",
      "codex",
      "s1",
      "v0",
      "http://localhost:1455/callback?code=xyz&state=s1",
    ],
  ]);
  assert.equal(state.callback.value, "", "the pasted link is cleared");
});

function quotaBlock(
  t: TestContext,
  read: (...args: any[]) => Promise<unknown>,
  account: Record<string, unknown> = {},
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaAccountQuota.vue",
    {
      ...shared,
      "../../api/oauth-accounts": { getCpaAccountQuota: read },
      "../../utils/oauth-display": {
        quotaNumber,
        quotaPercent,
        quotaResetLabel,
      },
    },
  );
  const props = reactive({
    provider: provider("gateway"),
    apiVersion: "v8",
    account: {
      name: "person@example.test.json",
      authIndex: "idx-1",
      quotaPluginId: "sso",
      supportsQuota: true,
      ...account,
    },
  });
  return { ...mount(t, component, props), props };
}

test("a CPA credential's quota is read only when asked, through its own plugin", async (t) => {
  const reads: unknown[][] = [];
  const { state } = quotaBlock(t, (...args: unknown[]) => {
    reads.push(args);
    return Promise.resolve({
      plan: "Pro",
      tier: "Tier 2",
      metrics: [
        {
          key: "balance",
          label: "余额",
          value: 12.5,
          unit: "USD",
          format: "currency",
          currency: "USD",
        },
      ],
      groups: [
        {
          displayName: "Claude",
          buckets: [
            {
              window: "5h",
              remainingFraction: 0.4,
              resetAt: null,
              description: "",
            },
          ],
        },
      ],
    });
  });
  await settle();
  assert.deepEqual(reads, [], "expanding a row never queries the provider");
  assert.equal(state.quota.value, null);
  await state.load();
  assert.deepEqual(reads, [["gateway", "idx-1", "sso", "v8"]]);
  assert.equal(state.plan.value, "Pro");
  assert.equal(state.quota.value.groups[0].buckets[0].remainingFraction, 0.4);
  assert.equal(state.usedPercent(state.quota.value.groups[0].buckets[0]), 60);
  assert.equal(state.remainingText(state.quota.value.groups[0].buckets[0]), "剩 40%");
});

test("a bucket without a fraction leaves the meter empty instead of full", async (t) => {
  const { state } = quotaBlock(t, async () => ({
    metrics: [],
    groups: [
      {
        displayName: "",
        buckets: [{ window: "weekly", remainingFraction: null, resetAt: 1_767_225_600 }],
      },
    ],
  }));
  await state.load();
  const bucket = state.quota.value.groups[0].buckets[0];
  assert.equal(state.usedPercent(bucket), null);
  assert.equal(state.remainingText(bucket), "");
  assert.equal(state.bucketLabel(bucket), "weekly");
});

test("a refused quota keeps the gateway's reason and releases the button", async (t) => {
  const { state } = quotaBlock(t, async () => {
    throw new Error("该插件不提供额度查询");
  });
  await state.load();
  assert.equal(state.loading.value, false);
  assert.equal(state.error.value, "Error: 该插件不提供额度查询");
  assert.equal(state.quota.value, null);
});

test("a quota answer that lands after another credential is selected is dropped", async (t) => {
  const reading = deferred<any>();
  const { state, props } = quotaBlock(t, () => reading.promise);
  const pending = state.load();
  props.account = {
    name: "other@example.test.json",
    authIndex: "idx-2",
    quotaPluginId: "sso",
    supportsQuota: true,
  };
  await settle();
  reading.resolve({ plan: "Pro", metrics: [], groups: [] });
  await pending;
  assert.equal(state.quota.value, null, "the previous credential's reading is not shown");
  assert.equal(state.loading.value, false);
});

function pluginStore(
  t: TestContext,
  api: Record<string, (...args: any[]) => Promise<unknown>>,
  extra: Record<string, unknown> = {},
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaPluginStore.vue",
    {
      ...shared,
      "../../api/oauth-accounts": {
        listCpaPluginStore: async () => ({
          enabled: true,
          plugins: [],
          sourceErrors: [],
        }),
        installCpaPlugin: async () => ({
          id: "",
          version: "",
          restartRequired: false,
        }),
        ...api,
      },
      ...extra,
    },
  );
  const props = reactive({ provider: provider("gateway"), revision: 2 });
  return { ...mount(t, component, props), props };
}

function storePlugin(overrides: Record<string, unknown> = {}) {
  return {
    id: "quota",
    sourceId: "official",
    sourceName: "官方",
    name: "Quota",
    description: "读取额度",
    author: "example",
    version: "1.2.0",
    repository: "example/quota",
    tags: ["quota"],
    authRequired: true,
    authConfigured: false,
    installed: true,
    installedVersion: "1.1.0",
    updateAvailable: true,
    ...overrides,
  };
}

test("the plugin store lists entries together with the sources that failed", async (t) => {
  const { state } = pluginStore(t, {
    listCpaPluginStore: async () => ({
      enabled: false,
      plugins: [
        storePlugin(),
        storePlugin({ id: "login", name: "Login", tags: ["auth"] }),
      ],
      sourceErrors: [
        { sourceId: "mirror", sourceName: "镜像", message: "registry timeout" },
      ],
    }),
  });
  await settle();
  assert.equal(state.store.value.plugins.length, 2);
  assert.equal(state.store.value.enabled, false);
  assert.equal(
    state.sourceFailure(state.store.value.sourceErrors[0]),
    "镜像 读取失败：registry timeout",
  );
  assert.equal(state.entries.value.length, 2);
  state.filter.value = "auth";
  assert.deepEqual(
    state.entries.value.map((entry: any) => entry.id),
    ["login"],
    "tags take part in the search",
  );
  state.filter.value = "quota";
  assert.deepEqual(
    state.entries.value.map((entry: any) => entry.id),
    ["quota"],
  );
  state.filter.value = "nothing matches";
  assert.equal(state.entries.value.length, 0);
});

test("installing names the source and version, reports the restart and rereads the store", async (t) => {
  const calls: unknown[][] = [];
  let reads = 0;
  const { state, events } = pluginStore(t, {
    listCpaPluginStore: async () => {
      reads++;
      return { enabled: true, plugins: [storePlugin()], sourceErrors: [] };
    },
    installCpaPlugin: (...args: unknown[]) => {
      calls.push(args);
      return Promise.resolve({
        id: "quota",
        version: "1.2.0",
        restartRequired: true,
      });
    },
  });
  await settle();
  assert.equal(reads, 1);
  await state.install(state.store.value.plugins[0]);
  assert.deepEqual(calls, [["gateway", "quota", "official", "1.2.0", 2]]);
  assert.match(state.notice.value, /重启网关后生效/);
  assert.equal(state.installing.value, "");
  assert.equal(reads, 2, "the installed copy is read back from the gateway");
  assert.deepEqual(events, [["installed"]]);
});

test("declining the install confirmation leaves the gateway untouched", async (t) => {
  const calls: unknown[][] = [];
  const { state } = pluginStore(
    t,
    {
      listCpaPluginStore: async () => ({
        enabled: true,
        plugins: [storePlugin()],
        sourceErrors: [],
      }),
      installCpaPlugin: (...args: unknown[]) => {
        calls.push(args);
        return Promise.resolve({});
      },
    },
    {
      "../../composables/provider-credential-dialogs": {
        confirmAction: async () => false,
      },
    },
  );
  await settle();
  await state.install(state.store.value.plugins[0]);
  assert.deepEqual(calls, []);
  assert.equal(state.installing.value, "");
});

test("an install that finishes after another gateway is selected is dropped", async (t) => {
  const writing = deferred<any>();
  const { state, props } = pluginStore(t, {
    listCpaPluginStore: async () => ({
      enabled: true,
      plugins: [storePlugin()],
      sourceErrors: [],
    }),
    installCpaPlugin: () => writing.promise,
  });
  await settle();
  const pending = state.install(state.store.value.plugins[0]);
  await settle();
  props.provider = provider("gateway-2");
  await settle();
  writing.resolve({ id: "quota", version: "1.2.0", restartRequired: false });
  await pending;
  assert.equal(
    state.notice.value,
    "",
    "the previous gateway's install is not reported on this card",
  );
  assert.equal(state.installing.value, "");
});

test("removing a plugin asks first, writes the revision and rereads the list", async (t) => {
  const removals: unknown[][] = [];
  let reads = 0;
  const props = reactive({
    visible: true,
    provider: provider("gateway"),
    revision: 7,
  });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaPluginsDialog.vue",
    {
      ...shared,
      "../ContentEditor.vue": {},
      "./CpaPluginStore.vue": {},
      "../../api/oauth-accounts": {
        listCpaPlugins: async () => {
          reads++;
          return {
            enabled: true,
            plugins: [
              {
                id: "oauth-login",
                name: "OAuth Login",
                version: "1.0.0",
                enabled: true,
                effectiveEnabled: true,
                configured: true,
                registered: true,
                supportsQuota: false,
              },
            ],
          };
        },
        removeCpaPlugin: (id: string, pluginId: string, revision: number) => {
          removals.push([id, pluginId, revision]);
          return Promise.resolve({
            fileDeleted: true,
            configuredRemoved: true,
          });
        },
      },
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  assert.equal(reads, 1);
  await state.remove(state.plugins.value.plugins[0]);
  assert.deepEqual(removals, [["gateway", "oauth-login", 7]]);
  assert.equal(reads, 2, "the gateway list is read again after the removal");
  assert.equal(state.removing.value, null);
});

test("a declined removal never reaches the gateway or drops the plugin", async (t) => {
  const removals: unknown[][] = [];
  const props = reactive({
    visible: true,
    provider: provider("gateway"),
    revision: 1,
  });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaPluginsDialog.vue",
    {
      ...shared,
      "../ContentEditor.vue": {},
      "./CpaPluginStore.vue": {},
      "../../composables/provider-credential-dialogs": {
        confirmAction: async () => false,
      },
      "../../api/oauth-accounts": {
        listCpaPlugins: async () => ({
          enabled: true,
          plugins: [
            {
              id: "oauth-login",
              name: "OAuth Login",
              version: "",
              enabled: true,
              effectiveEnabled: true,
              configured: false,
              registered: true,
              supportsQuota: false,
            },
          ],
        }),
        removeCpaPlugin: (...args: unknown[]) => {
          removals.push(args);
          return Promise.resolve({});
        },
      },
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  await state.remove(state.plugins.value.plugins[0]);
  assert.deepEqual(removals, [], "a declined removal never reaches the gateway");
  assert.equal(state.plugins.value.plugins.length, 1);
  assert.equal(state.removing.value, null);
});

function requestLogDialog(
  t: TestContext,
  props: Record<string, unknown>,
  bindings: Record<string, unknown>,
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaRequestLogDialog.vue",
    { ...shared, ...bindings },
  );
  return mount(t, component, props);
}

test("CPA request logs read the switch first and a closed dialog never shows the read", async (t) => {
  const settings = deferred<CpaLogSettings>();
  const props = reactive({ provider: provider("gateway"), visible: true });
  let reads = 0;
  const { state } = requestLogDialog(t, props, {
    "../../api/oauth-accounts": {
      getCpaLogSettings: () => settings.promise,
      readCpaRequestLogs: () => {
        reads++;
        return new Promise<never>(() => {});
      },
    },
  });
  await settle();
  assert.equal(state.loading.value, true);
  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false, "closing releases the busy state");
  settings.resolve({ enabled: true });
  await settle();
  assert.equal(reads, 0, "a closed dialog never asks the gateway for the log");
  assert.equal(state.tail.value, null);
  assert.equal(state.error.value, "");
});

test("CPA request logs skip the read while the gateway writes none", async (t) => {
  const props = reactive({ provider: provider("gateway"), visible: true });
  let reads = 0;
  const { state } = requestLogDialog(t, props, {
    "../../api/oauth-accounts": {
      getCpaLogSettings: async () => ({ enabled: false }),
      readCpaRequestLogs: () => {
        reads++;
        return Promise.resolve(null);
      },
    },
  });
  await settle();
  assert.deepEqual(state.settings.value, { enabled: false });
  assert.equal(reads, 0, "a disabled log is explained by the switch, not by an empty read");
  assert.equal(state.tail.value, null);
  assert.equal(state.loading.value, false);
});

test("CPA log follow appends new lines and restarts the buffer after the gateway rotates", async (t) => {
  const first = deferred<CpaLogTail>();
  const second = deferred<CpaLogTail>();
  const third = deferred<CpaLogTail>();
  const cursors: string[] = [];
  const props = reactive({ provider: provider("gateway"), visible: true });
  const { state } = requestLogDialog(t, props, {
    "../../api/oauth-accounts": {
      getCpaLogSettings: async () => ({ enabled: true }),
      readCpaRequestLogs: (_id: string, _limit: number, cursor = "") => {
        cursors.push(cursor);
        if (!cursor) return first.promise;
        return cursor === "c1" ? second.promise : third.promise;
      },
    },
  });
  await settle();
  first.resolve({
    lines: ["a", "b"],
    lineCount: 2,
    latestAt: null,
    nextCursor: "c1",
    cursorReset: false,
  });
  await settle();
  assert.deepEqual(state.tail.value.lines, ["a", "b"]);

  const polling = state.poll();
  second.resolve({
    lines: ["c"],
    lineCount: 3,
    latestAt: null,
    nextCursor: "c2",
    cursorReset: false,
  });
  await polling;
  assert.deepEqual(state.tail.value.lines, ["a", "b", "c"]);
  assert.equal(state.tail.value.nextCursor, "c2");

  const rotated = state.poll();
  third.resolve({
    lines: ["x"],
    lineCount: 1,
    latestAt: null,
    nextCursor: "c3",
    cursorReset: true,
  });
  await rotated;
  assert.deepEqual(state.tail.value.lines, ["x"], "a rotated log replaces the buffer");
  assert.deepEqual(cursors, ["", "c1", "c2"]);
});

test("CPA error logs drop a read that outlived the dialog and release the spinner", async (t) => {
  const listing = deferred<CpaErrorLog[]>();
  const props = reactive({ provider: provider("gateway"), visible: true });
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/oauth/CpaErrorLogDialog.vue",
    {
      ...shared,
      "../../utils/file-size": { formatFileSize: (value: number) => `${value}` },
      "../../api/oauth-accounts": {
        listCpaErrorLogs: () => listing.promise,
      },
    },
  );
  const { state } = mount(t, component, props);
  await settle();
  assert.equal(state.loading.value, true);
  props.visible = false;
  await settle();
  assert.equal(state.loading.value, false, "closing releases the busy state");
  listing.resolve([{ name: "fixture.log", size: 12, modifiedAt: null }]);
  await settle();
  assert.deepEqual(state.logs.value, [], "a closed dialog never shows the late list");
});

test("CPA log filtering narrows the visible lines without dropping the buffer", async (t) => {
  const props = reactive({ provider: provider("gateway"), visible: true });
  const { state } = requestLogDialog(t, props, {
    "../../api/oauth-accounts": {
      getCpaLogSettings: async () => ({ enabled: true }),
      readCpaRequestLogs: async () => ({
        lines: ["GET /a 200", "POST /b 500", "GET /c 200"],
        lineCount: 3,
        latestAt: null,
        nextCursor: "",
        cursorReset: false,
      }),
    },
  });
  await settle();
  assert.equal(state.shown.value.length, 3);
  state.filter.value = " 500 ";
  assert.deepEqual(
    state.shown.value.map((line: any) => line.raw),
    ["POST /b 500"],
    "the filter trims and matches a part of a line",
  );
  state.filter.value = "";
  assert.equal(state.shown.value.length, 3, "clearing the filter restores every buffered line");
});
