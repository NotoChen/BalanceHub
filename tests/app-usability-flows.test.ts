import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test, { type TestContext } from "node:test";
import { effectScope, reactive, ref, type Ref } from "vue";
import { createPinia } from "pinia";
import { useLatestRequest } from "../src/composables/useLatestRequest.ts";
import { withTimeout } from "../src/utils/promise-timeout.ts";
import * as announcementDisplay from "../src/utils/site-announcements.ts";
import * as providerRevision from "../src/utils/provider-revision.ts";
import { emptyDraft, providerToInput } from "../src/utils/provider-input.ts";
import { normalizeProviderBaseUrl } from "../src/composables/provider-editor-shared.ts";
import * as providerDisplay from "../src/utils/provider-display.ts";
import * as providerProtocol from "../src/utils/provider-protocol.ts";
import * as apiKeySettings from "../src/utils/provider-api-key-settings.ts";
import * as apiKeyOptions from "../src/utils/provider-api-key-options.ts";
import { keyEditorContext } from "./helpers/api-key-fixture.ts";
import { loadSource } from "./helpers/load-source.ts";
import type { AppSettings, AuthMode, Provider, ProviderAuthFieldDescriptor, ProviderAuthModeDescriptor, SiteAnnouncement, SiteAnnouncementsSnapshot } from "../src/stores/provider-types.ts";
import type { AppDataTransferResult, NotificationSendResult } from "../src/api/app.ts";

const require = createRequire(import.meta.url);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function scoped<T>(t: TestContext, create: () => T) {
  const scope = effectScope();
  t.after(() => scope.stop());
  return { value: scope.run(create)!, stop: () => scope.stop() };
}
async function settle() { for (let index = 0; index < 16; index++) await Promise.resolve(); }
function messages() {
  const sent: string[] = [];
  return { sent, Message: { success: (value: string) => sent.push(value), error: (value: string) => sent.push(value), warning: (value: string) => sent.push(value) } };
}

test("authentication sections stay visible and retain credentials across supported modes", (t) => {
  const component = loadSource<{ default: { setup: (props: object, context: object) => {
    selectMode: (mode: AuthMode) => void;
    updateField: (field: ProviderAuthFieldDescriptor, value: string) => void;
    credentialSections: Ref<ProviderAuthModeDescriptor[]>;
    sharedFields: Ref<ProviderAuthFieldDescriptor[]>;
    currentAuthMode: Ref<ProviderAuthModeDescriptor | undefined>;
    showAuthModePicker: Ref<boolean>;
    showCredentialPanel: Ref<boolean>;
  } } }>("components/provider-editor/ProviderEditorCredentialsSection.vue", {
    vue: { ...require("vue"), inject: () => undefined, useId: () => "auth-fixture" },
    "../../composables/useLoginAccounts": { LOGIN_ACCOUNTS_CONTEXT: Symbol("login") },
    "../../composables/useProviderCredentials": { PROVIDER_CREDENTIALS_CONTEXT: Symbol("credentials") },
    "@arco-design/web-vue/es/icon": {},
    "../../utils/provider-protocol": providerProtocol,
    "../ProviderAuthIcon.vue": {},
    "./ProviderCredentialFields.vue": {},
    "./ProviderApiKeyVault.vue": {},
  }).default;
  for (const protocol of ["newApi", "sub2Api", "api"] as const) {
    const draft = reactive(emptyDraft());
    draft.identity.protocol = protocol;
    Object.assign(draft.auth, {
      mode: protocol === "newApi" ? "session" : protocol === "sub2Api" ? "accessToken" : "apiKey",
      sessionCookie: "session=fixture-cookie", apiUser: "42",
      accessToken: "fixture-access-token", refreshToken: "fixture-refresh-token", accessTokenExpiresAt: 4102444800000,
      loginUsername: "fixture-user", loginPassword: "fixture-password", apiKey: "sk-fixture-key",
      newApiSession: protocol === "newApi" ? { accessToken: "fixture-session-token", accessExpiresAt: 4102444800, refreshCookie: "fixture-session-refresh", sessionId: "fixture-session" } : null,
    });
    const original = JSON.parse(JSON.stringify(draft.auth));
    const modes: AuthMode[] = protocol === "newApi" ? ["password", "session", "accessToken", "apiKey"] : protocol === "sub2Api" ? ["password", "accessToken", "apiKey"] : ["apiKey"];
    const fieldNames: Record<AuthMode, string[]> = {
      password: protocol === "newApi" ? ["loginUsername", "loginPassword", "apiUser"] : ["loginUsername", "loginPassword"],
      session: ["sessionCookie", "apiUser"],
      accessToken: protocol === "newApi" ? ["accessToken", "apiUser"] : ["accessToken", "refreshToken"],
      apiKey: ["apiKey"],
    };
    const required: Record<AuthMode, string[]> = {
      password: ["loginUsername", "loginPassword"],
      session: [],
      accessToken: protocol === "newApi" ? ["accessToken", "apiUser"] : ["accessToken"],
      apiKey: ["apiKey"],
    };
    const schemas = modes.map((mode): ProviderAuthModeDescriptor => ({
      mode, label: mode, description: "", note: "", requiredFields: required[mode],
      requiredAnyFields: mode === "session" ? ["sessionCookie", "newApiSession.refreshCookie"] : [], optionalFields: [],
      fields: fieldNames[mode].map((field) => ({ field, label: field, placeholder: "", secret: false, wide: false, readonly: mode === "password" && field === "apiUser", showWhenEmpty: true })),
    }));
    const props = reactive({ draft, disabled: false, apiKeyOptions: [], apiKeyManagerProvider: null as Provider | null, providerProtocols: [{ kind: protocol, authModes: schemas, browserLoginSupported: protocol === "newApi" }] });
    const { value: panel } = scoped(t, () => component.setup(props, { expose() {}, emit() {} }));
    const expectedFields = [...new Set(schemas.flatMap((mode) => mode.fields.map((field) => field.field)))].sort();
    for (const mode of [...modes, ...modes.toReversed()]) {
      panel.selectMode(mode);
      for (const field of ["loginUsername", "loginPassword", "accessToken", "sessionCookie"] as const) {
        panel.updateField({ field, readonly: false } as ProviderAuthFieldDescriptor, original[field]);
      }
      assert.deepEqual(JSON.parse(JSON.stringify(draft.auth)), { ...original, mode }, `${protocol}: ${mode}`);
      assert.deepEqual(panel.credentialSections.value.map((section) => section.mode), modes);
      assert.deepEqual([
        ...panel.credentialSections.value.flatMap((section) => section.fields.map((field) => field.field)),
        ...panel.sharedFields.value.map((field) => field.field),
      ].sort(), expectedFields, "every field is visible once, regardless of the selected mode");
      assert.deepEqual(panel.currentAuthMode.value?.requiredFields, required[mode]);
    }
    assert.equal(panel.showAuthModePicker.value, protocol !== "api");
    assert.deepEqual(panel.sharedFields.value.map((field) => field.field), protocol === "newApi" ? ["apiUser"] : []);
    if (protocol === "newApi") assert.equal(panel.sharedFields.value[0]?.readonly, false);
    draft.id = "fixture-provider";
    props.apiKeyManagerProvider = { identity: { id: draft.id } } as Provider;
    panel.selectMode("apiKey");
    assert.deepEqual(panel.credentialSections.value.map((section) => section.mode), modes.filter((mode) => mode !== "apiKey"), "saved API Keys appear in the existing vault without hiding account credentials");
    assert.equal(panel.showCredentialPanel.value, protocol !== "api", "Key-only providers do not gain an empty credential panel");
    props.disabled = true;
    panel.selectMode("password");
    assert.equal(draft.auth.mode, "apiKey");
  }
});

function setupKeyForm(t: TestContext, overrides: Record<string, unknown> = {}) {
  let mounted: (() => void) | undefined;
  const component = loadSource<{ default: { setup: (props: object, context: object) => {
    draft: Ref<import("../src/stores/provider-types.ts").ProviderApiKeySettings | null>;
    allowIps: Ref<string>;
    loading: ReturnType<typeof useLatestRequest>;
    saving: ReturnType<typeof useLatestRequest>;
    error: Ref<string>;
    save: () => Promise<void>;
  } } }>("components/provider-editor/ProviderApiKeyEditorForm.vue", {
    vue: { ...require("vue"), onMounted: (callback: () => void) => { mounted = callback; } },
    "@arco-design/web-vue/es/icon": {},
    "../../composables/useLatestRequest": { useLatestRequest },
    "../../utils/provider-api-key-settings": apiKeySettings,
  }).default;
  const emitted: unknown[][] = [];
  const props = { editing: true, isActive: () => true, loadContext: async () => keyEditorContext(), submit: async () => "saved", ...overrides };
  const scope = scoped(t, () => component.setup(props, { expose() {}, emit: (...args: unknown[]) => emitted.push(args) }));
  mounted?.();
  return { ...scope, emitted };
}

test("API Key form can close during context loading and ignores its late result", async (t) => {
  const pending = deferred<ReturnType<typeof keyEditorContext>>();
  const form = setupKeyForm(t, { loadContext: () => pending.promise });
  await settle();
  assert.equal(form.value.loading.loading.value, true);
  form.stop();
  assert.equal(form.value.loading.loading.value, false);
  pending.resolve(keyEditorContext());
  await settle();
  assert.equal(form.value.draft.value, null);
  assert.deepEqual(form.emitted, []);
});

test("API Key form submits only changes and retains the draft after save failure", async (t) => {
  const sent: unknown[] = [];
  const form = setupKeyForm(t, { submit: async (patch: unknown, revision: number) => { sent.push(patch, revision); throw new Error("站点拒绝当前分组"); } });
  await settle();
  form.value.draft.value!.group = "group-b";
  form.value.allowIps.value = "10.0.0.0/8\n";
  await form.value.save();
  assert.deepEqual(sent, [{ group: "group-b" }, 4]);
  assert.equal(form.value.saving.loading.value, false);
  assert.equal(form.value.draft.value!.group, "group-b");
  assert.match(form.value.error.value, /当前分组/);
});

test("API Key save timeout releases the form and discards a late success", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const pending = deferred<string>();
  const form = setupKeyForm(t, { submit: () => pending.promise });
  await settle();
  form.value.draft.value!.name = "changed";
  const saving = form.value.save();
  await settle();
  assert.equal(form.value.saving.loading.value, true);
  t.mock.timers.tick(65_000);
  await saving;
  assert.equal(form.value.saving.loading.value, false);
  assert.match(form.value.error.value, /同步站点 Key 确认/);
  pending.resolve("late");
  await settle();
  assert.deepEqual(form.emitted, []);
});

test("API Key dialog cancellation settles immediately while its native request is pending", async () => {
  let modal!: { onCancel: () => void; content: () => { props: { onSaved: (result: unknown) => void; isActive: () => boolean } } };
  const { openApiKeyEditor } = loadSource<typeof import("../src/composables/provider-api-key-editor.ts")>("composables/provider-api-key-editor.ts", {
    "@arco-design/web-vue": { Modal: { open: (options: typeof modal) => { modal = options; return { close() {} }; } } },
    "../components/provider-editor/ProviderApiKeyEditorForm.vue": { default: { name: "KeyForm" } },
  });
  const pending = deferred<string>();
  const editor = openApiKeyEditor({ editing: true, loadContext: async () => keyEditorContext(), submit: () => pending.promise });
  const form = modal.content().props;
  modal.onCancel();
  assert.equal(await editor.result, null);
  assert.equal(form.isActive(), false);
  form.onSaved("late");
  assert.equal(await editor.result, null);
  pending.resolve("late");
});

test("API Key remote editor leaves the main vault available and closing it prevents stale feedback", async (t) => {
  const feedback = messages();
  const pending = deferred<null>();
  let closed = false;
  const { useApiKeyManager } = loadSource<typeof import("../src/composables/useApiKeyManager.ts")>("composables/useApiKeyManager.ts", {
    "@arco-design/web-vue": feedback,
    "../utils/provider-display": providerDisplay,
    "../utils/provider-api-key-options": apiKeyOptions,
    "./useClipboard": {},
    "../utils/promise-timeout": { withTimeout },
    "./provider-api-key-editor": { openApiKeyEditor: () => ({ result: pending.promise, close: () => { closed = true; pending.resolve(null); } }) },
  });
  const current = { ...provider("key-manager"), auth: { apiKey: "", apiKeyOptions: [] }, actions: { apiKeyManagement: true } } as Provider;
  const { value: manager } = scoped(t, () => useApiKeyManager({ providers: ref([current]), getProvider: () => current } as Parameters<typeof useApiKeyManager>[0]));
  manager.bindApiKeyManager(current);
  manager.openApiKeyCreateEditor();
  assert.equal(manager.apiKeyManagerOperation.value, null);
  manager.closeApiKeyManager();
  assert.equal(closed, true);
  await settle();
  assert.equal(manager.apiKeyManagerProvider.value, null);
  assert.deepEqual(feedback.sent, []);
});

test("API Key updates merge current credentials and revision without overwriting provider draft edits", (t) => {
  const { useProviderEditorState } = loadSource<typeof import("../src/composables/useProviderEditorState.ts")>("composables/useProviderEditorState.ts", {
    "../utils/provider-input": { emptyDraft, providerToInput },
    "../utils/provider-api-key-options": apiKeyOptions,
    "./provider-editor-shared": { normalizeProviderBaseUrl },
  });
  const initial = emptyDraft();
  const stored = {
    ...initial,
    identity: { ...initial.identity, id: "fixture", baseUrl: "https://relay.example.invalid" },
    auth: { ...initial.auth, apiKey: "sk-first", accessToken: "fixture-old-token", loginPassword: "fixture-old-password", credentialRevision: 4 },
    capabilities: { availableModels: ["first-key-model"] },
  } as Provider;
  const { value: editor } = scoped(t, useProviderEditorState);
  editor.openEditProvider(stored);
  editor.draftProvider.identity.name = "Unsaved station name";
  editor.draftProvider.auth.loginPassword = "fixture-user-edited-password";
  const refreshed = structuredClone(stored);
  refreshed.auth.apiKey = "sk-second";
  refreshed.auth.accessToken = "fixture-rotated-token";
  refreshed.auth.loginPassword = "fixture-server-password";
  refreshed.auth.credentialRevision = 5;
  refreshed.capabilities.availableModels = [];
  editor.syncManagedApiKeys(refreshed);
  assert.equal(editor.draftProvider.identity.name, "Unsaved station name");
  assert.equal(editor.draftProvider.auth.loginPassword, "fixture-user-edited-password");
  assert.equal(editor.draftProvider.auth.accessToken, "fixture-rotated-token");
  assert.equal(editor.draftProvider.auth.apiKey, "sk-second");
  assert.equal(editor.draftProvider.auth.credentialRevision, 5);
  assert.deepEqual(editor.availableModels.value, []);
  refreshed.auth.accessToken = "fixture-next-token";
  editor.syncManagedApiKeys(refreshed);
  assert.equal(editor.draftProvider.auth.accessToken, "fixture-next-token");
  assert.equal(editor.draftProvider.auth.loginPassword, "fixture-user-edited-password");
  editor.openAddProvider();
  editor.syncManagedApiKeys(refreshed);
  assert.equal(editor.draftProvider.auth.apiKey, "");
  assert.equal(editor.draftProvider.auth.credentialRevision, 0);
});
const provider = (id: string) => ({ identity: { id, protocol: "newApi", baseUrl: `https://${id}.example.invalid` }, auth: { mode: "password" }, runtime: { enabled: true } }) as Provider;

test("password changes remain closable, reject repeat clicks and ignore completion from a previous dialog", async (t) => {
  const feedback = messages();
  const { usePasswordChange } = loadSource<typeof import("../src/composables/usePasswordChange.ts")>("composables/usePasswordChange.ts", {
    "@arco-design/web-vue": feedback, "./useLatestRequest": { useLatestRequest },
  });
  const old = deferred<string>();
  const fresh = deferred<string>();
  let calls = 0;
  const { value: panel } = scoped(t, () => usePasswordChange({ providers: ref([provider("a"), provider("b")]), changePassword: () => ++calls === 1 ? old.promise : fresh.promise }));
  panel.openPasswordChange(provider("a"));
  const first = panel.submitPasswordChange("", "test-only-value");
  await panel.submitPasswordChange("", "ignored");
  panel.passwordChangeVisible.value = false;
  assert.equal(panel.passwordChangeLoading.value, false);
  panel.openPasswordChange(provider("b"));
  const second = panel.submitPasswordChange("", "test-only-value");
  old.resolve("old result");
  await first;
  assert.equal(calls, 2);
  assert.equal(panel.passwordChangeVisible.value, true);
  assert.equal(panel.passwordChangeLoading.value, true);
  assert.deepEqual(feedback.sent, []);
  fresh.reject(new Error("站点拒绝修改"));
  await second;
  assert.equal(panel.passwordChangeLoading.value, false);
  assert.equal(panel.passwordChangeError.value, "站点拒绝修改");
  assert.equal(panel.passwordChangeVisible.value, true);
});

test("notification tests use a snapshot and never publish obsolete channel results", async (t) => {
  const feedback = messages();
  const old = deferred<NotificationSendResult>();
  const fresh = deferred<NotificationSendResult>();
  const inputs: AppSettings[] = [];
  const { useSystemNotification } = loadSource<typeof import("../src/composables/useSystemNotification.ts")>("composables/useSystemNotification.ts", {
    "@arco-design/web-vue": feedback, "./useLatestRequest": { useLatestRequest },
    "../api/app": { sendAppNotification: (value: AppSettings, _title: string, _message: string, ignoreSwitch: boolean) => {
      assert.equal(ignoreSwitch, true);
      inputs.push(value);
      return inputs.length === 1 ? old.promise : fresh.promise;
    } },
  });
  const draft = reactive({ notificationEnabled: false, notificationChannels: [{ id: "system", name: "系统通知", kind: "system", enabled: true, url: "", secret: "" }], proxyMode: "system", proxyUrl: "" } as AppSettings);
  const { value: panel } = scoped(t, () => useSystemNotification(draft));
  const first = panel.sendTestNotification();
  await panel.sendTestNotification();
  assert.equal(inputs.length, 1);
  draft.notificationChannels[0].name = "修改后的名称";
  assert.equal(panel.testingNotification.value, false);
  const second = panel.sendTestNotification();
  old.resolve({ sentCount: 0, results: [] });
  await first;
  assert.equal(inputs[0].notificationChannels[0].name, "系统通知");
  assert.equal(panel.notificationTestResult.value, null);
  assert.deepEqual(feedback.sent, []);
  fresh.resolve({ sentCount: 1, results: [{ channelId: "system", channelName: "修改后的名称", channelKind: "system", ok: true, message: "" }] });
  await second;
  assert.equal(panel.notificationTestResult.value?.sentCount, 1);
  assert.equal(panel.testingNotification.value, false);
  assert.equal(feedback.sent.length, 1);
});

test("notification timeout releases the button and late success cannot erase the error", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const pending = deferred<NotificationSendResult>();
  const feedback = messages();
  const { useSystemNotification } = loadSource<typeof import("../src/composables/useSystemNotification.ts")>("composables/useSystemNotification.ts", {
    "@arco-design/web-vue": feedback, "./useLatestRequest": { useLatestRequest }, "../api/app": { sendAppNotification: () => pending.promise },
  });
  const { value: panel } = scoped(t, () => useSystemNotification(reactive({ notificationChannels: [] } as unknown as AppSettings)));
  const sending = panel.sendTestNotification();
  await settle();
  t.mock.timers.tick(45_000);
  await sending;
  assert.equal(panel.testingNotification.value, false);
  assert.match(panel.notificationTestError.value, /超时/);
  pending.resolve({ sentCount: 1, results: [] });
  await settle();
  assert.equal(panel.notificationTestResult.value, null);
  assert.deepEqual(feedback.sent, []);
});

function announcement(id: string, readAt: string | null = null): SiteAnnouncement {
  return { fingerprint: id, providerId: "a", id, title: id, content: id, publishedAt: "10", updatedAt: null, readAt, canMarkRead: true } as SiteAnnouncement;
}
function announcementHarness(t: TestContext, api: { getSiteAnnouncements: () => Promise<SiteAnnouncementsSnapshot>; markSiteAnnouncementRead: () => Promise<unknown> }) {
  const feedback = messages();
  let reloads = 0;
  const { useSiteAnnouncements } = loadSource<typeof import("../src/composables/useSiteAnnouncements.ts")>("composables/useSiteAnnouncements.ts", {
    "@arco-design/web-vue": feedback, "../utils/promise-timeout": { withTimeout }, "../utils/site-announcements": announcementDisplay, "../api/app": api,
  });
  const context = scoped(t, () => useSiteAnnouncements({ providers: ref([provider("a")]), initialized: ref(false), reloadProviders: async () => { reloads++; } }));
  return { ...context, feedback, reloads: () => reloads };
}

test("background announcement reads do not mark content read, opening selects the first unread item", async (t) => {
  let marks = 0;
  const context = announcementHarness(t, {
    getSiteAnnouncements: async () => ({ fetchedAt: "1", announcements: [announcement("read", "1"), announcement("unread")], errors: [] }),
    markSiteAnnouncementRead: async () => { marks++; },
  });
  await context.value.refreshSiteAnnouncements();
  assert.equal(marks, 0);
  assert.equal(context.value.unreadSiteAnnouncementCount.value, 1);
  assert.equal(context.value.selectedSiteAnnouncement.value, null);
  context.value.openSiteAnnouncements();
  assert.equal(context.value.selectedSiteAnnouncement.value?.fingerprint, "unread");
  assert.equal(context.value.unreadSiteAnnouncementCount.value, 0);
  await settle();
  assert.equal(marks, 1);
});

test("announcement disposal releases loading and rejects late read and mark-all callbacks", async (t) => {
  const pending = deferred<SiteAnnouncementsSnapshot>();
  const sync = deferred<void>();
  const context = announcementHarness(t, { getSiteAnnouncements: () => pending.promise, markSiteAnnouncementRead: () => sync.promise });
  const reading = context.value.refreshSiteAnnouncements();
  context.stop();
  assert.equal(context.value.siteAnnouncementsLoading.value, false);
  pending.resolve({ fetchedAt: "1", announcements: [announcement("old")], errors: [] });
  await reading;
  assert.deepEqual(context.value.siteAnnouncements.value, []);
  assert.equal(context.reloads(), 0);

  const marking = announcementHarness(t, { getSiteAnnouncements: async () => ({ fetchedAt: "1", announcements: [announcement("a")], errors: [] }), markSiteAnnouncementRead: () => sync.promise });
  await marking.value.refreshSiteAnnouncements();
  const all = marking.value.markAllSiteAnnouncementsRead();
  marking.stop();
  sync.reject(new Error("late failure"));
  await all;
  assert.deepEqual(marking.feedback.sent, []);
  assert.equal(marking.reloads(), 1, "only the initial successful read should reload providers");
});

const transfer = (): AppDataTransferResult => ({ path: "/unused-memory-backup.json", schemaVersion: 1, providerCount: 2 });
function transferHarness(t: TestContext, dialogs: Record<string, unknown>, overrides: Partial<Parameters<typeof import("../src/composables/useAppDataTransfer.ts").useAppDataTransfer>[0]> = {}, confirm = async () => true) {
  const feedback = messages();
  const { useAppDataTransfer } = loadSource<typeof import("../src/composables/useAppDataTransfer.ts")>("composables/useAppDataTransfer.ts", {
    "@arco-design/web-vue": feedback, "@tauri-apps/plugin-dialog": dialogs,
    "./provider-credential-dialogs": { confirmAction: confirm }, "../utils/promise-timeout": { withTimeout },
  });
  const operations: string[] = [];
  const context = scoped(t, () => useAppDataTransfer({ beforeTransfer: async () => { operations.push("save settings"); return true; }, exportAppData: async () => { operations.push("export"); return transfer(); }, importAppData: async () => { operations.push("restore"); return transfer(); }, ...overrides }));
  return { ...context, feedback, operations };
}

test("backup dialogs cannot multiply and cancellation releases both actions", async (t) => {
  const dialog = deferred<string | null>();
  let dialogs = 0;
  const context = transferHarness(t, { save: () => { dialogs++; return dialog.promise; }, open: () => { dialogs++; return null; } });
  const exporting = context.value.exportAppData();
  await context.value.exportAppData();
  await context.value.importAppData();
  assert.equal(dialogs, 1);
  assert.equal(context.value.exportingAppData.value, true);
  dialog.resolve(null);
  await exporting;
  assert.equal(context.value.exportingAppData.value, false);
  assert.deepEqual(context.operations, []);
});

test("restoring requires confirmation and successful settings persistence before the native write", async (t) => {
  const confirmation = deferred<boolean>();
  const context = transferHarness(t, { open: async () => "/unused-memory-backup.json" }, {}, () => confirmation.promise);
  const restoring = context.value.importAppData();
  await settle();
  assert.deepEqual(context.operations, []);
  confirmation.resolve(true);
  await restoring;
  assert.deepEqual(context.operations, ["save settings", "restore"]);
  assert.equal(context.value.importingAppData.value, false);

  const failed = transferHarness(t, { open: async () => "/unused-memory-backup.json" }, { beforeTransfer: async () => false });
  await failed.value.importAppData();
  assert.deepEqual(failed.operations, []);
  assert.equal(failed.value.importingAppData.value, false);
  assert.match(failed.feedback.sent[0], /尚未保存/);
});

test("backup timeout, native dialog failure and disposal all release busy state", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const native = deferred<AppDataTransferResult>();
  const context = transferHarness(t, { open: async () => "/unused-memory-backup.json" }, { importAppData: () => native.promise });
  const restoring = context.value.importAppData();
  await settle();
  t.mock.timers.tick(30_000);
  await restoring;
  assert.equal(context.value.importingAppData.value, false);
  assert.match(context.feedback.sent[0], /超时/);
  native.resolve(transfer());
  await settle();
  assert.equal(context.feedback.sent.length, 1, "late completion must not show another success toast");
  const failed = transferHarness(t, { save: async () => { throw new Error("无法打开文件选择器"); } });
  await failed.value.exportAppData();
  assert.equal(failed.value.exportingAppData.value, false);
  const dialog = deferred<string>();
  const disposed = transferHarness(t, { save: () => dialog.promise });
  const exporting = disposed.value.exportAppData();
  disposed.stop();
  dialog.resolve("/unused-memory-backup.json");
  await exporting;
  assert.equal(disposed.value.exportingAppData.value, false);
  assert.deepEqual(disposed.operations, []);
});

test("prompt preview uses the current draft and invalidates immediately after edits or closing", async (t) => {
  const pending = deferred<string[]>();
  const feedback = messages();
  const component = loadSource<{ default: { setup: (props: { settings: AppSettings }, context: { expose: () => void }) => { refreshPromptPreviews: () => Promise<void>; promptPreviews: { value: string[] }; preview: ReturnType<typeof useLatestRequest> } } }>("components/settings/SettingsLivenessPromptSection.vue", {
    "@arco-design/web-vue": feedback,
    "../../api/app": { previewLivenessPrompts: () => pending.promise },
    "../../composables/provider-credential-dialogs": { confirmAction: async () => false },
    "../../composables/useLatestRequest": { useLatestRequest },
    "../../stores/providers": { defaultSettings: () => ({}) },
    "../../utils/liveness-options": { livenessPromptModeOptions: [] },
  }).default;
  const settings = reactive({ livenessPromptMode: "fixed", livenessFixedPrompt: "before", livenessPromptLibrary: [], livenessPlaceholderPools: [], livenessNumberMin: 1, livenessNumberMax: 10 } as unknown as AppSettings);
  const context = scoped(t, () => component.setup({ settings }, { expose() {} }));
  const reading = context.value.refreshPromptPreviews();
  await settle();
  settings.livenessFixedPrompt = "after";
  assert.equal(context.value.preview.loading.value, false);
  pending.resolve(["before"]);
  await reading;
  assert.deepEqual(context.value.promptPreviews.value, []);
  context.stop();
  await context.value.refreshPromptPreviews();
  assert.deepEqual(context.value.promptPreviews.value, []);
});

test("loading local configuration is retriable and does not wait for runtime discovery", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const native = deferred<unknown>();
  const runtime = deferred<void>();
  let loads = 0;
  const { useProviderStore } = loadSource<typeof import("../src/stores/providers.ts")>("stores/providers.ts", {
    "../api/app": { loadAppData: async () => { if (++loads === 1) throw new Error("temporary failure"); if (loads === 2) return native.promise; return { providers: [], providerProtocols: [], settings: {}, workspaces: [], temporaryCliPreferences: [], revision: 1 }; } },
    "../utils/provider-input": { providerToInput }, "../utils/provider-revision": providerRevision,
    "../utils/promise-timeout": { withTimeout }, "./provider-defaults": { defaultSettings: () => ({}) },
    "./cli-runtime": { useCliRuntimeStore: () => ({ refresh: () => runtime.promise }) },
    "./settings": { useSettingsStore: () => ({ hydrate() {} }) }, "./workspaces": { useWorkspaceStore: () => ({ hydrate() {} }) },
  });
  const store = useProviderStore(createPinia());
  t.after(() => store.$dispose());
  await store.initialize();
  assert.equal(store.loadError, "temporary failure");
  assert.equal(store.loading, false);
  const retry = store.initialize();
  await store.initialize();
  assert.equal(loads, 2, "a retry must be single-flight");
  t.mock.timers.tick(15_000);
  await retry;
  assert.equal(store.loading, false);
  assert.match(store.loadError!, /超时/);
  await store.initialize();
  assert.equal(store.loadError, null);
  assert.equal(store.loading, false, "local data is ready while runtime discovery remains pending");
  native.resolve({ providers: [provider("stale")], revision: 0 });
  await settle();
  assert.deepEqual(store.providers, []);
  runtime.resolve();
});

function providerOrderStore(t: TestContext, reorderProviders: (ids: string[]) => Promise<string[]>) {
  const { useProviderStore } = loadSource<typeof import("../src/stores/providers.ts")>("stores/providers.ts", {
    "../api/app": { reorderProviders },
    "../utils/provider-input": { providerToInput }, "../utils/provider-revision": providerRevision,
    "../utils/promise-timeout": { withTimeout }, "./provider-defaults": { defaultSettings: () => ({}) },
    "./cli-runtime": { useCliRuntimeStore: () => ({}) }, "./settings": { useSettingsStore: () => ({}) },
    "./workspaces": { useWorkspaceStore: () => ({}) },
  });
  const store = useProviderStore(createPinia());
  store.replaceProviders([provider("a"), provider("b"), provider("c")]);
  t.after(() => store.$dispose());
  return store;
}

test("provider order ignores an older response after a newer drag has finished", async (t) => {
  const old = deferred<string[]>();
  const fresh = deferred<string[]>();
  let calls = 0;
  const store = providerOrderStore(t, () => ++calls === 1 ? old.promise : fresh.promise);
  const first = store.reorderProviders(["b", "a", "c"]);
  const second = store.reorderProviders(["c", "b", "a"]);
  fresh.resolve(["c", "b", "a"]);
  await second;
  old.resolve(["b", "a", "c"]);
  await first;
  assert.deepEqual(store.providers.map((item) => item.identity.id), ["c", "b", "a"]);
});

test("provider order times out, ignores late success, and allows an immediate retry", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const late = deferred<string[]>();
  let calls = 0;
  const store = providerOrderStore(t, async (ids) => ++calls === 1 ? late.promise : ids);
  const saving = store.reorderProviders(["b", "a", "c"]);
  const failure = assert.rejects(saving, /排序超时/);
  t.mock.timers.tick(15_000);
  await failure;
  late.resolve(["b", "a", "c"]);
  await settle();
  assert.deepEqual(store.providers.map((item) => item.identity.id), ["a", "b", "c"]);
  await store.reorderProviders(["c", "a", "b"]);
  assert.deepEqual(store.providers.map((item) => item.identity.id), ["c", "a", "b"]);
});

test("large model lists expose every page and searching resets to the first matching page", async (t) => {
  const component = loadSource<{ default: { setup: (props: object, context: object) => { keyword: Ref<string>; page: Ref<number>; displayedModels: Ref<string[]>; filteredModels: Ref<string[]>; canRefresh: Ref<boolean>; modalTitle: Ref<string> } } }>("components/AvailableModelsModal.vue", {
    "@arco-design/web-vue/es/icon": {}, "../utils/provider-display": providerDisplay,
  }).default;
  const names = Array.from({ length: 620 }, (_, index) => `model-${String(index).padStart(3, "0")}`);
  const props = reactive({ visible: true, provider: { ...modelProvider("a"), capabilities: { ...modelProvider("a").capabilities, availableModels: names } }, loading: false, error: "" });
  const { value: panel } = scoped(t, () => component.setup(props, { expose() {}, emit() {} }));
  assert.equal(panel.canRefresh.value, true);
  assert.match(panel.modalTitle.value, /账号可用模型/);
  assert.equal(panel.displayedModels.value.length, 100);
  panel.page.value = 7;
  assert.equal(panel.displayedModels.value.length, 20);
  assert.equal(panel.displayedModels.value.at(-1), "model-619");
  panel.keyword.value = "model-61";
  await settle();
  assert.equal(panel.page.value, 1);
  assert.equal(panel.filteredModels.value.length, 10);
  assert.deepEqual(panel.displayedModels.value, panel.filteredModels.value);
});

function modelProvider(id: string): Provider {
  return {
    ...provider(id),
    displayLabel: `Fixture ${id}`,
    auth: { mode: "session", apiKey: "", credentialRevision: 0 },
    capabilities: { availableModels: [], availableModelsState: { scope: null, updatedAt: null, error: null } },
    actions: { models: { canSync: true, scope: "account", unavailableReason: null } },
  } as Provider;
}

function loadModelPanel(feedback: ReturnType<typeof messages>) {
  return loadSource<typeof import("../src/composables/useAvailableModels.ts")>("composables/useAvailableModels.ts", {
    "@arco-design/web-vue": feedback,
    "./useLatestRequest": { useLatestRequest },
    "./useClipboard": { copyText: async () => {} },
  }).useAvailableModels;
}

test("account models load without a Key, remain closable and ignore a closed dialog's late result", async (t) => {
  const feedback = messages();
  const useAvailableModels = loadModelPanel(feedback);
  const first = deferred<import("../src/stores/provider-types.ts").ProviderModelSyncResult>();
  const second = deferred<import("../src/stores/provider-types.ts").ProviderModelSyncResult>();
  const providers = ref([modelProvider("a"), modelProvider("b")]);
  const calls: string[] = [];
  const { value: panel } = scoped(t, () => useAvailableModels({ providers, syncModels: (id) => {
    calls.push(id);
    return calls.length === 1 ? first.promise : second.promise;
  } }));
  panel.openAvailableModels(providers.value[0]);
  await settle();
  assert.deepEqual(calls, ["a"]);
  assert.equal(panel.availableModelsLoading.value, true);
  panel.availableModelsVisible.value = false;
  assert.equal(panel.availableModelsLoading.value, false);
  panel.openAvailableModels(providers.value[1]);
  await settle();
  first.resolve({ provider: providers.value[0], models: ["old"], message: "obsolete" });
  await settle();
  assert.equal(panel.availableModelsProvider.value?.identity.id, "b");
  assert.equal(panel.availableModelsLoading.value, true);
  assert.deepEqual(feedback.sent, []);
  second.reject(new Error("模型接口暂不可用"));
  await settle();
  assert.equal(panel.availableModelsLoading.value, false);
  assert.equal(panel.availableModelsError.value, "模型接口暂不可用");
  assert.equal(panel.availableModelsVisible.value, true);
});

test("a successful empty model list stays cached and the backend owns refresh availability", async (t) => {
  const feedback = messages();
  const useAvailableModels = loadModelPanel(feedback);
  const value = modelProvider("a");
  value.capabilities.availableModelsState = { scope: "account", updatedAt: "123", error: null };
  let calls = 0;
  const { value: panel } = scoped(t, () => useAvailableModels({ providers: ref([value]), syncModels: async () => {
    calls += 1;
    return { provider: value, models: [], message: "已获取 0 个模型" };
  } }));
  panel.openAvailableModels(value);
  await settle();
  assert.equal(calls, 0);
  await panel.refreshAvailableModels();
  assert.equal(calls, 1);
  panel.availableModelsProvider.value!.actions.models.canSync = false;
  panel.availableModelsProvider.value!.actions.models.unavailableReason = "请先登录账号";
  await panel.refreshAvailableModels();
  assert.equal(calls, 1);
  assert.equal(panel.availableModelsError.value, "请先登录账号");
});

test("model request timeout releases busy state and late success cannot replace its error", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const feedback = messages();
  const useAvailableModels = loadModelPanel(feedback);
  const value = modelProvider("a");
  const pending = deferred<import("../src/stores/provider-types.ts").ProviderModelSyncResult>();
  const { value: panel } = scoped(t, () => useAvailableModels({ providers: ref([value]), syncModels: () => pending.promise }));
  panel.openAvailableModels(value);
  await settle();
  t.mock.timers.tick(45_000);
  await settle();
  assert.equal(panel.availableModelsLoading.value, false);
  assert.match(panel.availableModelsError.value, /超时/);
  pending.resolve({ provider: value, models: ["late"], message: "obsolete" });
  await settle();
  assert.match(panel.availableModelsError.value, /超时/);
  assert.deepEqual(feedback.sent, []);
});

test("changing account credentials invalidates pending model feedback", async (t) => {
  const feedback = messages();
  const useAvailableModels = loadModelPanel(feedback);
  const providers = ref([modelProvider("a")]);
  const pending = deferred<import("../src/stores/provider-types.ts").ProviderModelSyncResult>();
  const { value: panel } = scoped(t, () => useAvailableModels({ providers, syncModels: () => pending.promise }));
  panel.openAvailableModels(providers.value[0]);
  await settle();
  providers.value[0].auth.credentialRevision += 1;
  assert.equal(panel.availableModelsLoading.value, false);
  pending.reject(new Error("old credentials"));
  await settle();
  assert.equal(panel.availableModelsError.value, "");
  assert.deepEqual(feedback.sent, []);
});

test("model probe failures stay in their own step and a successful empty list clears the warning", (t) => {
  const component = loadSource<{ default: { setup: (props: object, context: object) => {
    steps: Ref<Array<{ key: string; status: string; detail: string }>>;
    overallTone: Ref<string>;
  } } }>("components/CapabilityProbeModal.vue", {
    "@arco-design/web-vue/es/icon": {},
    "../utils/provider-display": providerDisplay,
    "../utils/provider-protocol": providerProtocol,
  }).default;
  const value = modelProvider("a");
  value.capabilities.availableModelsState = { scope: "account", updatedAt: "123", error: "HTTP 503" };
  const props = reactive({ visible: true, provider: value, providerProtocols: [], running: false,
    error: "", resultMessage: "", startedAt: 1, finishedAt: 2 });
  const { value: panel } = scoped(t, () => component.setup(props, { expose() {}, emit() {} }));
  assert.equal(panel.overallTone.value, "partial");
  assert.equal(panel.steps.value.find((step) => step.key === "models")?.status, "error");
  assert.ok(panel.steps.value.filter((step) => step.key !== "models").every((step) => step.status !== "error"));
  props.provider.capabilities.availableModelsState.error = null;
  assert.equal(panel.overallTone.value, "success");
  const modelStep = panel.steps.value.find((step) => step.key === "models")!;
  assert.equal(modelStep.status, "supported");
  assert.match(modelStep.detail, /当前列表为空/);
});

test("reopening request logs clears an unsubmitted search and the previously selected record", async (t) => {
  const display = loadSource("utils/request-log-display.ts", { "./provider-display": providerDisplay });
  const component = loadSource<{ default: { setup: (props: object, context: object) => { keywordDraft: Ref<string>; selectedLog: Ref<unknown>; submitSearch: (event?: { isComposing?: boolean; keyCode?: number }) => void } } }>("components/RequestLogsModal.vue", {
    "@arco-design/web-vue/es/icon": {}, "../utils/provider-display": providerDisplay,
    "../utils/request-log-display": display, "./RequestLogDetailsModal.vue": {},
  }).default;
  const emitted: unknown[][] = [];
  const props = reactive({ visible: true, provider: provider("a"), keyword: "", page: 0, pageSize: 20, loading: false, error: "", result: null });
  const { value: panel } = scoped(t, () => component.setup(props, { expose() {}, emit: (...args: unknown[]) => emitted.push(args) }));
  panel.keywordDraft.value = "尚未提交";
  panel.selectedLog.value = { requestId: "old-record" };
  panel.submitSearch({ isComposing: true });
  assert.deepEqual(emitted, [], "confirming Chinese input must not submit a search");
  props.visible = false;
  props.visible = true;
  assert.equal(panel.keywordDraft.value, "");
  assert.equal(panel.selectedLog.value, null);
});

test("small displayed usage stays nonzero and honors the station currency", () => {
  assert.equal(providerDisplay.formatQuotaValue(0.005, { quotaDisplayType: "currency", currencySymbol: "$" }, 6), "$0.005");
  assert.equal(providerDisplay.formatQuotaValue(0.000002, { quotaDisplayType: "currency", currencySymbol: "€" }, 6), "€0.000002");
  assert.equal(providerDisplay.formatQuotaValue(0, { quotaDisplayType: "currency", currencySymbol: "$" }, 6), "$0");
});

test("finishing onboarding saves the current draft through the same queue without blocking navigation", async (t) => {
  const feedback = messages();
  const { useOnboardingController } = loadSource<typeof import("../src/composables/useOnboardingController.ts")>("composables/useOnboardingController.ts", { "@arco-design/web-vue": feedback });
  const saved = { onboardingCompleted: false, agentCliPaths: {}, themeMode: "light" } as AppSettings;
  const draft = reactive({ ...saved, themeMode: "dark" } as AppSettings);
  const pending = deferred<boolean>();
  let saves = 0;
  let opened = 0;
  const { value: panel } = scoped(t, () => useOnboardingController({
    initialized: ref(true), loadError: ref(null), providers: ref([]), settings: ref(saved), settingsForm: draft,
    flushSettingsSave: () => { saves++; assert.equal(draft.onboardingCompleted, true); assert.equal(draft.themeMode, "dark"); return pending.promise; },
    importAppData: async () => {}, openAddProvider() {}, openSettings: () => { opened++; },
  }));
  const saving = panel.completeOnboarding();
  await panel.completeOnboarding();
  assert.equal(saves, 1);
  assert.equal(panel.onboardingVisible.value, false);
  panel.openOnboardingSettings();
  assert.equal(opened, 1, "the main interface stays usable before persistence completes");
  pending.resolve(true);
  await saving;
  assert.equal(draft.themeMode, "dark");
});

test("onboarding save failures retain edits and release the action for retry", async (t) => {
  const feedback = messages();
  const { useOnboardingController } = loadSource<typeof import("../src/composables/useOnboardingController.ts")>("composables/useOnboardingController.ts", { "@arco-design/web-vue": feedback });
  const draft = reactive({ onboardingCompleted: false, agentCliPaths: {}, themeMode: "dark" } as AppSettings);
  let saves = 0;
  const { value: panel } = scoped(t, () => useOnboardingController({
    initialized: ref(true), loadError: ref(null), providers: ref([]), settings: ref({ ...draft }), settingsForm: draft,
    flushSettingsSave: async () => ++saves > 1,
    importAppData: async () => {}, openAddProvider() {}, openSettings() {},
  }));
  await panel.completeOnboarding();
  assert.equal(panel.onboardingVisible.value, true);
  assert.equal(draft.themeMode, "dark");
  await panel.completeOnboarding();
  assert.equal(saves, 2);
  assert.equal(panel.onboardingVisible.value, false);
});
