import assert from "node:assert/strict";
import test, { after, before } from "node:test";
import { createPinia, setActivePinia } from "pinia";
import { computed, createRenderer, createSSRApp, defineComponent, effectScope, h, nextTick, ref } from "vue";
import { renderToString } from "@vue/server-renderer";
import { createServer, type ViteDevServer } from "vite";
import { readFileSync } from "node:fs";
import type { AgentAssetOperation, AgentEnvironmentInventory } from "../src/stores/provider-types.ts";
import { catalogSnapshot } from "./agent-workspace-fixtures.ts";
import { assetAction, assetContext, assetInventory, assetOperation, assetPlan, assetPreview, assetRecord, assetSource } from "./agent-asset-fixtures.ts";

type StoreModule = typeof import("../src/stores/agent-environment.ts");
type CatalogModule = typeof import("../src/composables/useAgentAssetCatalog.ts");
type ConsoleModule = typeof import("../src/composables/useAgentAssetConsole.ts");
type Controller = ReturnType<ConsoleModule["useAgentAssetConsole"]>;
type Deferred = { promise: Promise<unknown>; resolve: (value: unknown) => void; reject: (error: unknown) => void };
let server: ViteDevServer;
let storeModule: StoreModule;
let catalogModule: CatalogModule;
let catalogStoreModule: typeof import("../src/stores/agent-catalog.ts");
let consoleModule: ConsoleModule;
const queue = new Map<string, Deferred[]>();
const calls: { command: string; args: Record<string, unknown> }[] = [];

before(async () => {
  Object.assign(globalThis, { ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } });
  const windowEvents = new EventTarget();
  const documentEvents = new EventTarget();
  Object.assign(globalThis, { addEventListener: windowEvents.addEventListener.bind(windowEvents), removeEventListener: windowEvents.removeEventListener.bind(windowEvents), document: { documentElement: { style: {} }, visibilityState: "visible", addEventListener: documentEvents.addEventListener.bind(documentEvents), removeEventListener: documentEvents.removeEventListener.bind(documentEvents) } });
  Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", { configurable: true, value: {
    transformCallback() { return 1; }, unregisterCallback() {},
    invoke(command: string, args: Record<string, unknown> = {}) {
      calls.push({ command, args });
      const deferred = queue.get(command)?.shift();
      if (!deferred && command === "get_agent_catalog_revision") return Promise.resolve(catalogStoreModule.useAgentCatalogStore().catalogs.__native__?.revision);
      if (!deferred) throw new Error(`Unexpected Tauri command: ${command}`);
      return deferred.promise;
    },
  } });
  server = await createServer({ optimizeDeps: { noDiscovery: true, include: [] }, server: { middlewareMode: true }, appType: "custom", logLevel: "silent" });
  storeModule = await server.ssrLoadModule("/src/stores/agent-environment.ts") as StoreModule;
  catalogModule = await server.ssrLoadModule("/src/composables/useAgentAssetCatalog.ts") as CatalogModule;
  catalogStoreModule = await server.ssrLoadModule("/src/stores/agent-catalog.ts");
  consoleModule = await server.ssrLoadModule("/src/composables/useAgentAssetConsole.ts") as ConsoleModule;
});
after(async () => { await server.close(); queue.clear(); });

function pending(command: string): Deferred {
  let resolve!: Deferred["resolve"];
  let reject!: Deferred["reject"];
  const promise = new Promise<unknown>((done, fail) => { resolve = done; reject = fail; });
  const deferred = { promise, resolve, reject };
  queue.set(command, [...(queue.get(command) ?? []), deferred]);
  return deferred;
}
async function settle() { for (let index = 0; index < 12; index += 1) await Promise.resolve(); await nextTick(); }
async function until(predicate: () => boolean) {
  const deadline = Date.now() + 3000;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error("Expected state was not reached");
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
}

function mountConsole(fixture = assetInventory()) {
  const pinia = createPinia();
  setActivePinia(pinia);
  queue.clear(); calls.length = 0;
  const store = storeModule.useAgentEnvironmentStore();
  store.inventories.__native__ = fixture;
  store.inventoryState.__native__ = "ready";
  catalogStoreModule.useAgentCatalogStore().catalogs.__native__ = catalogSnapshot({ inventory: fixture });
  const workspace = ref<string | undefined>();
  let controller!: Controller;
  const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
    patchProp() {}, insert() {}, remove() {}, createElement: (type) => ({ type }), createText: (text) => ({ text }), createComment: (text) => ({ text }),
    setText(node, text) { node.text = text; }, setElementText(node, text) { node.text = text; }, parentNode: () => null, nextSibling: () => null, querySelector: () => null, setScopeId() {}, cloneNode: (node) => ({ ...node }), insertStaticContent: () => [{}, {}],
  });
  const app = renderer.createApp(defineComponent({ setup() {
    controller = consoleModule.useAgentAssetConsole({ inventory: computed(() => store.inventory(workspace.value)), workspace });
    return () => h("div");
  } }));
  app.use(pinia); app.mount({});
  return { controller, store, workspace, app };
}

test("native details keep shared installation and profile provenance distinct", () => {
  const fixture = assetInventory();
  fixture.contexts.push(assetContext({ id: "context-b", profile: "review", configRoot: "/fixture/review", compatibleInstallationIds: ["installation-a"] }));
  fixture.sources.push(assetSource("profile-source", { contextId: "context-b", path: "/fixture/review/config.toml" }));
  fixture.assets.push(assetRecord("profile-asset", { contextId: "context-b", sourceIds: ["profile-source"], inspectionSourceId: "profile-source" }));
  const scope = effectScope();
  const catalog = scope.run(() => catalogModule.useAgentAssetCatalog(ref(fixture)))!;
  assert.equal(catalog.detailFor("asset-a")?.compatibleInstallations.length, 2);
  assert.equal(catalog.detailFor("profile-asset")?.row.context?.profile, "review");
  assert.equal(catalog.detailFor("profile-asset")?.row.sources[0].id, "profile-source");
  assert.ok(catalog.indexes.value.sources.has("source-only"));
  scope.stop();
});

test("detail keeps represented declarations, contributors, parent, owner and affected children separate", () => {
  const parent = assetRecord("parent", { category: "plugin", relationships: { providedBy: null, actionOwner: null, affectedAssetIds: ["child"] } });
  const child = assetRecord("child", { category: "skill", relationships: { providedBy: "parent", actionOwner: "parent", affectedAssetIds: [] }, actions: [assetAction("inspect"), assetAction("enable", { available: false, reason: "childOwnedByParent" })] });
  const fixture = assetInventory({ assets: [parent, child] });
  const overlay = { ...fixture.declarations[0], id: "overlay", declarationKey: "disabled-state", role: "stateOverlay" as const };
  fixture.declarations.push(overlay);
  parent.resolution.contributorIds.push("overlay");
  const scope = effectScope();
  const catalog = scope.run(() => catalogModule.useAgentAssetCatalog(ref(fixture)))!;
  const parentDetail = catalog.detailFor("parent")!;
  assert.deepEqual(parentDetail.children.map((item) => item.id), ["child"]);
  assert.deepEqual(parentDetail.affected.map((item) => item.id), ["child"]);
  assert.equal(parentDetail.declarations.find((item) => item.declaration.id === "overlay")?.represented, false);
  assert.equal(parentDetail.declarations.find((item) => item.declaration.id === "overlay")?.contributor, true);
  assert.equal(catalog.detailFor("child")?.provider?.id, "parent");
  assert.equal(catalog.detailFor("child")?.actionOwner?.id, "parent");
  assert.match(catalog.detailFor("child")!.row.readOnlyReason!, /父扩展/);
  scope.stop();
});

test("a fifth registry Agent renders through the generic catalog and safe icon fallback", async () => {
  const fixture = assetInventory();
  for (const item of [...fixture.installations, ...fixture.contexts, ...fixture.assets]) Reflect.set(item, "agentKind", "fixture-fifth-agent");
  fixture.installations[0].label = "Fixture Fifth Agent";
  const scope = effectScope();
  const catalog = scope.run(() => catalogModule.useAgentAssetCatalog(ref(fixture)))!;
  assert.equal(catalog.rows.value[0].agentLabel, "Fixture Fifth Agent");
  assert.equal(catalog.rows.value.length, 2);
  const module = await server.ssrLoadModule("/src/components/AgentCliIcon.vue");
  const html = await renderToString(createSSRApp(module.default, { kind: "fixture-fifth-agent", decorative: false, label: "Fixture Fifth Agent" }));
  assert.match(html, /agent-cli-icon-generic/);
  assert.match(html, /Fixture Fifth Agent/);
  scope.stop();
});

test("closing the plan during preparation prevents a late plan from reopening it", async () => {
  const { controller, app } = mountConsole();
  const plan = pending("plan_agent_asset");
  const preparation = controller.action("asset", "asset-a", "enable");
  assert.equal(controller.planVisible.value, true);
  assert.equal(controller.rowBusy("asset-a"), true);
  assert.equal(controller.rowBusy("asset-b"), false);
  controller.closePlan();
  plan.resolve(assetPlan());
  await preparation;
  assert.equal(controller.planVisible.value, false);
  assert.equal(controller.pendingPlan.value, null);
  assert.equal(controller.rowBusy("asset-a"), false);
  app.unmount();
});

test("confirming closes the modal immediately, leaves other rows usable and releases failed apply", async () => {
  const { controller, store, app } = mountConsole();
  const plan = pending("plan_agent_asset");
  const preparation = controller.action("asset", "asset-a", "enable");
  plan.resolve(assetPlan()); await preparation;
  const apply = pending("apply_agent_asset");
  controller.confirmPlan();
  assert.equal(controller.planVisible.value, false);
  assert.equal(controller.pendingPlan.value, null);
  assert.equal(controller.rowBusy("asset-a"), true);
  assert.equal(controller.rowBusy("asset-b"), false);
  controller.openDetail("asset-b");
  assert.equal(controller.selectedDetail.value?.row.asset.stableId, "asset-b");
  controller.closeDetail();
  assert.equal(controller.drawerVisible.value, false);
  apply.reject({ kind: "sourceConflict", message: "配置已变化，未执行修改" });
  await settle();
  assert.equal(controller.rowBusy("asset-a"), false);
  assert.match(store.operationErrors[storeModule.agentAssetTargetKey("asset-a")]!, /配置已变化/);
  assert.deepEqual(calls.find((call) => call.command === "apply_agent_asset")?.args, { request: { planToken: "plan:asset-a", assetId: "asset-a", action: "enable" } });
  app.unmount();
});

test("same-source rows can prepare independently while the first backend operation waits for its lock", async () => {
  const { controller, store, app } = mountConsole();
  const plan = pending("plan_agent_asset");
  const preparation = controller.action("asset", "asset-a", "enable");
  plan.resolve(assetPlan()); await preparation;
  const apply = pending("apply_agent_asset");
  const poll = pending("get_agent_asset_operation");
  controller.confirmPlan();
  apply.resolve(assetOperation());
  await until(() => Boolean(store.operations["operation-a"]));
  assert.equal(controller.rowProgress("asset-a"), "等待同源操作");
  assert.equal(controller.rowBusy("asset-b"), false);
  const otherPlan = pending("plan_agent_asset");
  const otherPreparation = controller.action("asset", "asset-b", "enable");
  assert.equal(controller.planningAssetId.value, "asset-b");
  controller.closePlan(); otherPlan.resolve(assetPlan("asset-b")); await otherPreparation;
  poll.resolve(assetOperation("operation-a", { phase: "completed", canCancel: false, revision: 2, outcome: "appliedVerified" }));
  await until(() => !controller.rowBusy("asset-a"));
  assert.equal(store.operations["operation-a"].outcome, "appliedVerified");
  assert.equal(controller.planVisible.value, false);
  app.unmount();
});

test("a bounded IPC timeout releases plan preparation without locking the modal or other rows", async () => {
  const { controller, app } = mountConsole();
  const timer = globalThis.setTimeout;
  globalThis.setTimeout = ((callback: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => timer(callback, delay === 20_000 ? 5 : delay, ...args)) as typeof setTimeout;
  try {
    const plan = pending("plan_agent_asset");
    await controller.action("asset", "asset-a", "enable");
    assert.equal(controller.rowBusy("asset-a"), false);
    assert.equal(controller.rowBusy("asset-b"), false);
    assert.match(controller.planError.value, /超时/);
    controller.closePlan();
    plan.resolve(assetPlan()); await settle();
    assert.equal(controller.pendingPlan.value, null);
  } finally { globalThis.setTimeout = timer; app.unmount(); }
});

test("operation status timeout releases row busy, preserves uncertainty and never retries apply", async () => {
  const { controller, store, app } = mountConsole();
  const timer = globalThis.setTimeout;
  globalThis.setTimeout = ((callback: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => timer(callback, delay === 20_000 ? 10 : delay, ...args)) as typeof setTimeout;
  try {
    const plan = pending("plan_agent_asset");
    const preparation = controller.action("asset", "asset-a", "enable");
    plan.resolve(assetPlan()); await preparation;
    const apply = pending("apply_agent_asset");
    const poll = pending("get_agent_asset_operation");
    controller.confirmPlan(); apply.resolve(assetOperation());
    await until(() => Boolean(store.operations["operation-a"]));
    await until(() => !controller.rowBusy("asset-a"));
    assert.match(controller.rowError("asset-a")!, /结果尚未确认/);
    assert.equal(store.operations["operation-a"].outcome, null);
    assert.equal(calls.filter((call) => call.command === "apply_agent_asset").length, 1);
    poll.resolve(assetOperation("operation-a", { phase: "completed", canCancel: false, revision: 3, outcome: "appliedVerified" }));
    await settle();
    assert.equal(store.operations["operation-a"].phase, "waitingForLock");
  } finally { globalThis.setTimeout = timer; app.unmount(); }
});

test("retained inventory permits preview until publication and workspace changes reject stale plans", async () => {
  const { controller, store, workspace, app } = mountConsole();
  const preview = pending("read_agent_environment_source");
  const reading = controller.action("source", "source-only", "preview");
  assert.equal(controller.previewState.value, "loading");
  const refresh = pending("get_agent_environment_inventory");
  const refreshing = store.loadInventory(undefined, true);
  preview.resolve(assetPreview("source-only")); await reading;
  assert.equal(controller.preview.value?.stableId, "source-only");
  assert.equal(controller.previewState.value, "ready");
  refresh.resolve(assetInventory()); await refreshing;
  assert.equal(controller.previewState.value, "error");
  assert.match(controller.previewError.value, /目录已更新/);
  const plan = pending("plan_agent_asset");
  const preparing = controller.action("asset", "asset-a", "enable");
  workspace.value = "/fixture/other-workspace";
  plan.resolve(assetPlan()); await preparing;
  assert.equal(controller.pendingPlan.value, null);
  assert.equal(controller.planVisible.value, false);
  assert.equal(controller.drawerVisible.value, false);
  assert.equal(controller.rowBusy("asset-a"), false);
  app.unmount();
});

test("publication invalidates previews requested while refresh retained the old inventory", async () => {
  for (const fails of [false, true]) {
    const { controller, store, app } = mountConsole();
    const refresh = pending("get_agent_environment_inventory");
    const refreshing = store.loadInventory(undefined, true);
    const preview = pending("read_agent_environment_source");
    const reading = controller.action("source", "source-only", "preview");
    assert.equal(controller.previewState.value, "loading");
    const replacement = assetInventory();
    const source = replacement.sources.find((source) => source.id === "source-only")!;
    source.access = { kind: "ready", accessId: "replacement-access" };
    source.revision = { ...source.revision, identity: "replacement-revision" };
    refresh.resolve(replacement); await refreshing;
    if (fails) preview.reject(new Error("obsolete preview failure"));
    else preview.resolve(assetPreview("source-only"));
    await reading;
    assert.equal(controller.selectedSource.value?.access.kind, "ready");
    assert.equal(controller.preview.value, null);
    assert.match(controller.previewError.value, /目录在读取期间发生变化/);
    assert.equal(controller.previewState.value, "error");
    assert.equal(controller.rowBusy("source-only"), false);
    app.unmount();
  }
});

test("closing a pending preview keeps the drawer closed and tolerates a late safe cache result", async () => {
  const { controller, store, app } = mountConsole();
  const preview = pending("read_agent_environment_source");
  const reading = controller.action("source", "source-only", "preview");
  controller.closeDetail();
  assert.equal(controller.drawerVisible.value, false);
  assert.equal(controller.rowBusy("asset-b"), false);
  preview.resolve(assetPreview("source-only")); await reading;
  assert.equal(controller.preview.value, null);
  assert.equal(controller.previewState.value, "idle");
  assert.equal(Object.values(store.previews)[0].stableId, "source-only");
  app.unmount();
});

test("each external open requires its own backend risks and passes only opaque access authority", async () => {
  const { controller, app } = mountConsole();
  await controller.action("asset", "asset-a", "open");
  assert.deepEqual(controller.accessConfirmation.value?.risks, ["externalPathnameRace", "rawSensitiveContent"]);
  assert.equal(calls.length, 0);
  const opening = pending("open_agent_environment_asset");
  controller.confirmAccess();
  assert.equal(controller.accessConfirmation.value, null);
  assert.equal(controller.rowBusy("asset-b"), false);
  opening.resolve(undefined); await settle();
  assert.deepEqual(calls[0].args, { request: { targetId: "asset-a", accessId: "access:asset-a", environmentId: "native:fixture", workspace: null, target: "asset", acceptedRisks: ["externalPathnameRace", "rawSensitiveContent"] } });
  assert.equal("path" in calls[0].args, false);
  await controller.action("asset", "asset-a", "open");
  assert.ok(controller.accessConfirmation.value);
  controller.closeAccessConfirmation();
  assert.equal(calls.length, 1);
  app.unmount();
});

test("stale access confirmation and mismatched preview responses are rejected", async () => {
  const { controller, store, workspace, app } = mountConsole();
  await controller.action("asset", "asset-a", "open");
  workspace.value = "/fixture/other";
  controller.confirmAccess();
  assert.equal(calls.length, 0);
  const reading = pending("read_agent_environment_source");
  const previewPromise = store.readConfigPreview("source-a", { accessId: "access:source-a", environmentId: "native:fixture" });
  reading.resolve(assetPreview("source-a", { accessId: "access:other" }));
  await assert.rejects(previewPromise, /访问凭据不一致/);
  assert.deepEqual(Object.values(store.previews), []);
  app.unmount();
});

test("operation status revisions and identity guards prevent stale or cross-target updates", () => {
  const { store, app } = mountConsole();
  const complete = assetOperation("operation-a", { phase: "completed", canCancel: false, revision: 4, outcome: "appliedUnverified" });
  store.acceptOperation(complete);
  store.acceptOperation(assetOperation("operation-a", { revision: 3 }));
  assert.equal(store.operations["operation-a"].phase, "completed");
  assert.throws(() => store.acceptOperation(assetOperation("wrong-id"), { assetId: "asset-a", action: "enable" }, "operation-a"), /编号不一致/);
  assert.throws(() => store.acceptOperation(assetOperation("operation-b", { assetId: "asset-b" }), { assetId: "asset-a", action: "enable" }), /资产不一致/);
  app.unmount();
});

test("completed uncertain operations are verified without applying their token again", async () => {
  const { controller, store, app } = mountConsole();
  const uncertain = assetOperation("operation-a", { phase: "completed", revision: 2, canCancel: false, outcome: "outcomeUnknown" });
  store.acceptOperation(uncertain);
  store.operationTargets[uncertain.id] = { assetId: uncertain.assetId, action: uncertain.action };
  const verification = pending("verify_agent_asset_operation");
  const verifying = controller.verifyOperation(uncertain.id);
  verification.resolve(assetOperation("operation-a", { phase: "completed", revision: 3, canCancel: false, outcome: "appliedVerified" }));
  await verifying;
  assert.equal(store.operations["operation-a"].outcome, "appliedVerified");
  assert.equal(calls.filter((call) => call.command === "apply_agent_asset").length, 0);
  app.unmount();
});

test("recovery restores backend operations into the same row state", async () => {
  const { controller, store, app } = mountConsole();
  const recovery = pending("list_agent_asset_operations");
  const recovered = controller.recoverOperations();
  const operation: AgentAssetOperation = assetOperation("restored", { phase: "completed", outcome: "appliedUnverified", canCancel: false });
  recovery.resolve([operation]); await recovered;
  assert.equal(controller.operationFor("asset-a")?.id, "restored");
  assert.equal(store.operationTargets.restored.workspace, undefined);
  assert.equal(controller.rowBusy("asset-a"), false);
  app.unmount();
});

test("the integrated console and detail surfaces compile and keep Hook navigation with its existing owner", async () => {
  for (const path of [
    "/src/components/agent-workspace/AgentDashboard.vue",
    "/src/components/agent-workspace/AgentCatalogPanel.vue",
    "/src/components/settings/agent-environment/AgentAssetDetailDrawer.vue",
    "/src/components/settings/agent-environment/AgentAssetPlanModal.vue",
    "/src/components/settings/agent-environment/AgentAssetAccessModal.vue",
  ]) assert.ok((await server.ssrLoadModule(path)).default);
  const root = readFileSync(new URL("../src/composables/useAgentDashboard.ts", import.meta.url), "utf8");
  assert.match(root, /useAgentHookConsole/);
  assert.match(root, /function showHook/);
  const state = readFileSync(new URL("../src/stores/agent-environment.ts", import.meta.url), "utf8");
  assert.doesNotMatch(state, /applyAgentHook|planAgentHook|inspectAgentHook/);
});
