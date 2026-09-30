import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test, { type TestContext } from "node:test";
import { effectScope, reactive, ref, type Ref } from "vue";
import { createPinia } from "pinia";
import { compileScript, parse } from "vue/compiler-sfc";
import ts from "typescript";
import { useLatestRequest } from "../src/composables/useLatestRequest.ts";
import { withTimeout } from "../src/utils/promise-timeout.ts";
import * as announcementDisplay from "../src/utils/site-announcements.ts";
import * as providerRevision from "../src/utils/provider-revision.ts";
import { providerToInput } from "../src/utils/provider-input.ts";
import * as providerDisplay from "../src/utils/provider-display.ts";
import type { AppSettings, Provider, SiteAnnouncement, SiteAnnouncementsSnapshot } from "../src/stores/provider-types.ts";
import type { AppDataTransferResult, NotificationSendResult } from "../src/api/app.ts";

const require = createRequire(import.meta.url);

// Execute the real source with native boundaries replaced in memory. No App,
// notification, file dialog, native configuration or desktop automation is used.
function loadSource<T>(path: string, bindings: Record<string, unknown>): T {
  const source = readFileSync(new URL(`../src/${path}`, import.meta.url), "utf8");
  const code = path.endsWith(".vue")
    ? compileScript(parse(source, { filename: path }).descriptor, { id: "settings-interaction-test" }).content
    : source;
  const output = ts.transpileModule(code, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports = {};
  new Function("require", "exports", output)((specifier: string) => {
    if (Object.hasOwn(bindings, specifier)) return bindings[specifier];
    if (specifier === "vue" || specifier === "pinia") return require(specifier);
    throw new Error(`Unmocked module: ${specifier}`);
  }, exports);
  return exports as T;
}

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
  const component = loadSource<{ default: { setup: (props: object, context: object) => { keyword: Ref<string>; page: Ref<number>; displayedModels: Ref<string[]>; filteredModels: Ref<string[]> } } }>("components/AvailableModelsModal.vue", {
    "@arco-design/web-vue/es/icon": {}, "../utils/provider-display": providerDisplay,
  }).default;
  const names = Array.from({ length: 620 }, (_, index) => `model-${String(index).padStart(3, "0")}`);
  const props = reactive({ visible: true, provider: { ...provider("a"), capabilities: { availableModels: names } }, loading: false, error: "" });
  const { value: panel } = scoped(t, () => component.setup(props, { expose() {}, emit() {} }));
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
