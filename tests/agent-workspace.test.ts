import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test, { after, before, type TestContext } from "node:test";
import { createPinia, disposePinia, setActivePinia, storeToRefs, type Pinia } from "pinia";
import { computed, createRenderer, createSSRApp, defineComponent, effectScope, h, nextTick, reactive, ref, type Component } from "vue";
import { renderToString } from "@vue/server-renderer";
import { createServer, type ViteDevServer } from "vite";
import type { AgentAssetCatalog, AgentCatalogPlan, AgentCatalogPlanRequest, AgentCatalogRelationIntent, AgentCatalogRelationPreview, AgentCatalogSaveRequest, AgentCatalogTarget } from "../src/stores/agent-catalog-types.ts";
import type { AgentLifecycleCatalog } from "../src/stores/agent-lifecycle-types.ts";
import type { AgentCliKind, AgentHookPlan, AgentRuntimeSnapshot, CliRuntimeSnapshot } from "../src/stores/provider-types.ts";
import type { AgentWorkspacePage } from "../src/stores/agent-workspace.ts";
import { assetAction, assetInstallation, assetInventory, assetOperation, assetRecord, assetSource } from "./agent-asset-fixtures.ts";
import { sessionPage, sessionScope } from "./agent-session-fixtures.ts";
import type { AgentSessionQuery } from "../src/stores/agent-session-types.ts";
import {
  catalogAgentPanel, catalogAsset, catalogBinding, catalogDefinition, catalogObservations, catalogOperation, catalogPlan, catalogProvenanceSnapshot, catalogRelationPreview, catalogSnapshot, catalogTarget,
  lifecycleCatalog, lifecycleOperation, lifecyclePlan, lifecycleTarget, workspaceAgent, workspaceHook,
} from "./agent-workspace-fixtures.ts";

type CatalogStoreModule = typeof import("../src/stores/agent-catalog.ts");
type LifecycleStoreModule = typeof import("../src/stores/agent-lifecycle.ts");
type EnvironmentModule = typeof import("../src/stores/agent-environment.ts");
type NavigationModule = typeof import("../src/stores/agent-workspace.ts");
type CatalogConsoleModule = typeof import("../src/composables/useAgentCatalogConsole.ts");
type LifecycleConsoleModule = typeof import("../src/composables/useAgentLifecycleConsole.ts");
type CatalogViewModule = typeof import("../src/composables/useAgentCatalogView.ts");
type EditorModule = typeof import("../src/composables/useAgentDefinitionEditor.ts");
type BackgroundModule = typeof import("../src/composables/useAgentBackgroundTasks.ts");
type DashboardModule = typeof import("../src/composables/useAgentDashboard.ts");
type SettingsModule = typeof import("../src/stores/settings.ts");
type CliModule = typeof import("../src/stores/cli-runtime.ts");
type Deferred = { promise: Promise<unknown>; resolve: (value: unknown) => void; reject: (error: unknown) => void };

let server: ViteDevServer;
let catalogStores: CatalogStoreModule;
let lifecycleStores: LifecycleStoreModule;
let environments: EnvironmentModule;
let navigations: NavigationModule;
let catalogConsoles: CatalogConsoleModule;
let lifecycleConsoles: LifecycleConsoleModule;
let catalogViews: CatalogViewModule;
let editors: EditorModule;
let backgrounds: BackgroundModule;
let dashboards: DashboardModule;
let settingsStores: SettingsModule;
let cliStores: CliModule;
let backendCatalog: AgentAssetCatalog;
let backendLifecycle: AgentLifecycleCatalog;
const queues = new Map<string, Deferred[]>();
const calls: { command: string; args: Record<string, unknown> }[] = [];
const unexpected: string[] = [];
const originalTimeout = globalThis.setTimeout;

before(async () => {
  Object.assign(globalThis, { ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } });
  const windowEvents = new EventTarget();
  const documentEvents = new EventTarget();
  Object.assign(globalThis, { addEventListener: windowEvents.addEventListener.bind(windowEvents), removeEventListener: windowEvents.removeEventListener.bind(windowEvents), document: { documentElement: { style: {} }, visibilityState: "visible", addEventListener: documentEvents.addEventListener.bind(documentEvents), removeEventListener: documentEvents.removeEventListener.bind(documentEvents) } });
  Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
  Object.assign(globalThis, { __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} } });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", { configurable: true, value: {
    transformCallback() { return 1; }, unregisterCallback() {},
    invoke(command: string, args: Record<string, unknown> = {}) {
      const { progress, requestId, ...payload } = args;
      if (progress !== undefined) assert.equal(typeof (progress as { id: unknown }).id, "number");
      if (requestId !== undefined) assert.equal(typeof requestId, "string");
      calls.push({ command, args: payload });
      const deferred = queues.get(command)?.shift();
      if (deferred) return deferred.promise;
      if (command === "count_agent_sessions") return Promise.resolve({ counts: [], errors: {} });
      if (command === "get_cached_cli_tools") return Promise.resolve(null);
      if (command === "probe_cli_tools") return Promise.resolve({ tools: [] });
      if (command === "has_agent_catalog_changes") return Promise.resolve(false);
      if (command === "list_provider_browser_logins") return Promise.resolve([]);
      if (command === "plugin:event|listen") return Promise.resolve(1);
      if (command === "plugin:event|unlisten") return Promise.resolve();
      if (command === "get_cached_agent_overview") return Promise.resolve([]);
      if (command === "refresh_agent_overview") return Promise.resolve({ agentKind: args.agentKind, probe: { ...workspaceAgent(args.agentKind as AgentCliKind), available: false, path: null, version: null, message: "fixture" }, configuration: null, configurationError: "", updatedAt: new Date().toISOString() });
      if (command === "get_agent_catalog_revision") return Promise.resolve(backendCatalog.revision);
      if (command === "get_agent_asset_catalog") return Promise.resolve(structuredClone(backendCatalog));
      if (command === "list_agent_configuration_sources") return Promise.resolve(null);
      if (command === "get_agent_lifecycle_catalog") return Promise.resolve(structuredClone(backendLifecycle));
      if (command === "get_agent_session_scope") return Promise.resolve(sessionScope(args.explicitWorkdir as string | null));
      if (command === "query_agent_sessions") return Promise.resolve(sessionPage([], { scopeRevision: (args.request as AgentSessionQuery).scopeRevision }));
      if (command === "cancel_agent_catalog_read" || command === "cancel_agent_session_query") return Promise.resolve();
      if (["list_agent_catalog_operations", "list_agent_lifecycle_operations", "list_agent_asset_operations", "list_agent_configuration_operations"].includes(command)) return Promise.resolve([]);
      if (command === "inspect_agent_hook" || command === "health_agent_hook") return Promise.resolve(workspaceHook(args.agentKind as AgentCliKind));
      unexpected.push(command);
      return Promise.reject(new Error(`Unexpected Tauri command: ${command}`));
    },
  } });
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { middlewareMode: true }, appType: "custom", logLevel: "silent",
    resolve: { alias: [
      { find: /^@arco-design\/web-vue$/, replacement: "virtual:agent-workspace-messages" },
      { find: /^@arco-design\/web-vue\/es\/icon$/, replacement: "virtual:agent-workspace-icons" },
    ] },
    plugins: [{ name: "agent-workspace-messages", resolveId(id) {
      if (id === "virtual:agent-workspace-messages" || id === "virtual:agent-workspace-icons") return `\0${id.slice("virtual:".length)}`;
    }, load(id) {
      if (id === "\0agent-workspace-messages") return "export const Message = { success() {}, error() {}, info() {}, warning() {} };";
      if (id === "\0agent-workspace-icons") return ["IconCopy", "IconLoading", "IconCheckCircle", "IconDelete", "IconMore", "IconRefresh", "IconSettings", "IconCommand", "IconApps", "IconBook", "IconBranch", "IconCode", "IconDashboard", "IconHistory", "IconLink", "IconThunderbolt"].map((name) => `export const ${name} = { render() { return null; } };`).join("\n");
    } }],
  });
  [catalogStores, lifecycleStores, environments, navigations, catalogConsoles, lifecycleConsoles, catalogViews, editors, backgrounds, dashboards, settingsStores, cliStores] = await Promise.all([
    server.ssrLoadModule("/src/stores/agent-catalog.ts"), server.ssrLoadModule("/src/stores/agent-lifecycle.ts"),
    server.ssrLoadModule("/src/stores/agent-environment.ts"), server.ssrLoadModule("/src/stores/agent-workspace.ts"),
    server.ssrLoadModule("/src/composables/useAgentCatalogConsole.ts"), server.ssrLoadModule("/src/composables/useAgentLifecycleConsole.ts"),
    server.ssrLoadModule("/src/composables/useAgentCatalogView.ts"), server.ssrLoadModule("/src/composables/useAgentDefinitionEditor.ts"),
    server.ssrLoadModule("/src/composables/useAgentBackgroundTasks.ts"), server.ssrLoadModule("/src/composables/useAgentDashboard.ts"),
    server.ssrLoadModule("/src/stores/settings.ts"), server.ssrLoadModule("/src/stores/cli-runtime.ts"),
  ]) as [CatalogStoreModule, LifecycleStoreModule, EnvironmentModule, NavigationModule, CatalogConsoleModule, LifecycleConsoleModule,
    CatalogViewModule, EditorModule, BackgroundModule, DashboardModule, SettingsModule, CliModule];
  globalThis.setTimeout = ((callback, delay, ...args) => originalTimeout(callback, delay === 800 ? 1 : delay, ...args)) as typeof setTimeout;
});

after(async () => { globalThis.setTimeout = originalTimeout; await server.close(); });

function pending(command: string): Deferred {
  let resolve!: Deferred["resolve"];
  let reject!: Deferred["reject"];
  const promise = new Promise<unknown>((done, fail) => { resolve = done; reject = fail; });
  const deferred = { promise, resolve, reject };
  queues.set(command, [...(queues.get(command) ?? []), deferred]);
  return deferred;
}
async function settle() { for (let index = 0; index < 16; index += 1) await Promise.resolve(); await nextTick(); }
async function until(predicate: () => boolean) {
  const deadline = Date.now() + 1500;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error("Expected state was not reached");
    await new Promise<void>((resolve) => originalTimeout(resolve, 2));
  }
}
async function fastTimeout<T>(milliseconds: number, run: () => Promise<T>) {
  const previous = globalThis.setTimeout;
  globalThis.setTimeout = ((callback, delay, ...args) => previous(callback, delay === milliseconds ? 8 : delay, ...args)) as typeof setTimeout;
  try { return await run(); } finally { globalThis.setTimeout = previous; }
}

function fresh(t: TestContext, snapshot = catalogSnapshot()) {
  queues.clear(); calls.length = 0; unexpected.length = 0;
  backendCatalog = snapshot; backendLifecycle = lifecycleCatalog();
  const pinia = createPinia(); setActivePinia(pinia);
  const catalog = catalogStores.useAgentCatalogStore(pinia);
  const lifecycle = lifecycleStores.useAgentLifecycleStore(pinia);
  const environment = environments.useAgentEnvironmentStore(pinia);
  const navigation = navigations.useAgentWorkspaceStore(pinia);
  const settings = settingsStores.useSettingsStore(pinia);
  const cli = cliStores.useCliRuntimeStore(pinia);
  catalog.catalogs = { __native__: snapshot };
  environment.inventories.__native__ = snapshot.inventory;
  environment.inventoryState.__native__ = "ready";
  lifecycle.catalog = backendLifecycle;
  const cleanup: (() => void)[] = [];
  function ui<T>(setup: () => T) {
    const scope = effectScope();
    const value = scope.run(setup)!;
    cleanup.push(() => scope.stop());
    return { value, stop: () => scope.stop() };
  }
  function mount<T>(setup: () => T) {
    let value!: T;
    const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
      patchProp() {}, insert() {}, remove() {}, createElement: (type) => ({ type }), createText: (text) => ({ text }), createComment: (text) => ({ text }),
      setText(node, text) { node.text = text; }, setElementText(node, text) { node.text = text; }, parentNode: () => null, nextSibling: () => null,
      querySelector: () => null, setScopeId() {}, cloneNode: (node) => ({ ...node }), insertStaticContent: () => [{}, {}],
    });
    const app = renderer.createApp(defineComponent({ setup() { value = setup(); return () => h("div"); } }));
    app.use(pinia); app.mount({}); cleanup.push(() => app.unmount());
    return { value, app };
  }
  t.after(async () => { cleanup.forEach((stop) => stop()); disposePinia(pinia); await settle(); assert.deepEqual(unexpected, []); queues.clear(); });
  return { pinia, catalog, lifecycle, environment, navigation, settings, cli, ui, mount };
}

function catalogUi(context: ReturnType<typeof fresh>) {
  const { workspacePath, navigationRevision } = storeToRefs(context.navigation);
  return context.ui(() => catalogConsoles.useAgentCatalogConsole({ workspace: workspacePath, navigationRevision }));
}
function lifecycleUi(context: ReturnType<typeof fresh>) {
  return context.ui(() => lifecycleConsoles.useAgentLifecycleConsole(storeToRefs(context.navigation).navigationRevision));
}
function viewProps(snapshot = catalogSnapshot()) {
  return reactive({ catalog: snapshot as AgentAssetCatalog | null, page: "mcp" as AgentWorkspacePage, query: "", agentFilter: null as AgentCliKind | null,
    agents: [workspaceAgent("codex"), workspaceAgent("claudeCode")] });
}
function provenanceView(t: TestContext, snapshot = catalogProvenanceSnapshot()) {
  const context = fresh(t, snapshot); const props = viewProps(snapshot);
  props.page = "skill"; props.agents.push(workspaceAgent("gemini"));
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  return { context, props, view };
}
async function renderSurface(path: string, props: Record<string, unknown>, pinia: Pinia) {
  const component = (await server.ssrLoadModule(path)).default as Component;
  if (path.endsWith("/AgentCatalogDetailDrawer.vue") && !props.resource) {
    const asset = props.asset as ReturnType<typeof catalogAsset>;
    const snapshot = props.catalog as AgentAssetCatalog;
    const parentIds = new Set(asset.bindings.map(binding => binding.native.relationships.providedBy));
    props = { ...props, resource: reactive({
      reading: true, managementExpanded: false, sharedView: false, binding: null, editor: { visible: false },
      reader: { loading: false, error: "", content: null, reload() {} },
      management: { panel: null, loading: false, error: "" },
      parents: snapshot.inventory.assets.filter(item => parentIds.has(item.stableId)), children: [],
    }) };
  }
  if (path.endsWith("/AgentOperationPanel.vue") && !props.selection) {
    const id = Object.keys(catalogStores.useAgentCatalogStore(pinia).operations)[0];
    props = { ...props, selection: id ? { kind: "catalog", id } : { kind: "lifecycle", id: Object.keys(lifecycleStores.useAgentLifecycleStore(pinia).operations)[0] ?? "missing" } };
  }
  const app = createSSRApp(component, props); app.use(pinia);
  for (const [name, tag] of [["a-button", "button"], ["a-select", "select"], ["a-option", "option"], ["a-modal", "section"], ["a-drawer", "section"], ["a-dropdown", "div"], ["a-doption", "button"], ["a-switch", "button"], ["a-tooltip", "span"], ["a-popover", "span"]]) {
    app.component(name, defineComponent({ setup(_, { attrs, slots }) { return () => h(tag, attrs, [slots.title?.(), slots.default?.(), slots.content?.()]); } }));
  }
  return renderToString(app);
}

test("main view switches preserve the Agent page, filter and search while the provider workflow remains wired", (t) => {
  const { navigation } = fresh(t);
  navigation.openPage("mcp", "codex"); navigation.query = "fixture"; navigation.selectWorkspace(" /fixture/project ");
  const revision = navigation.navigationRevision;
  navigation.setView("providers"); navigation.setView("agents");
  assert.equal(navigation.page, "mcp"); assert.equal(navigation.agentFilter, "codex"); assert.equal(navigation.query, "fixture");
  assert.equal(navigation.workspacePath, "/fixture/project"); assert.equal(navigation.navigationRevision, revision + 2);
  const workspace = readFileSync(new URL("../src/components/AppWorkspace.vue", import.meta.url), "utf8");
  assert.match(workspace, /<ProviderBoard\s+v-show=/);
  assert.match(workspace, /@refresh="emit\('refresh', \$event\)"/);
  assert.match(workspace, /@launch-temporary-cli="\(provider, cliKind\) => emit\('launchTemporaryCli', provider, cliKind\)"/);
  assert.match(workspace, /agentWorkspace\.view === "agents" \? agentWorkspace\.query : searchQuery\.value/);
  assert.match(workspace, /:active="agentWorkspace\.view === 'agents'"/);
});

test("one backend logical identity renders once with two bindings and survives Agent filtering", async (t) => {
  const context = fresh(t); const props = viewProps();
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  assert.equal(view.rows.value.length, 1); assert.equal(view.bindingsFor(view.rows.value[0]).length, 2);
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogPanel.vue", { ...props, loading: false, error: "", busy: () => false }, context.pinia);
  assert.equal((html.match(/data-global-asset-id="global-asset"/g) ?? []).length, 1);
  assert.equal((html.match(/class="agent-catalog-agent-control\b/g) ?? []).length, 2);
  props.agentFilter = "codex";
  assert.equal(view.rows.value[0].id, "global-asset"); assert.equal(view.bindingsFor(view.rows.value[0]).length, 1);
  props.agentFilter = null;
  assert.equal(view.bindingsFor(view.rows.value[0]).length, 2);
});

test("same-name backend assets remain distinct and filters use native effective state and safe fields", (t) => {
  const first = catalogAsset();
  const other = catalogAsset("separate-identity", { bindings: [catalogBinding("codex", "another-binding")] });
  other.bindings[0].native.effectiveState = "enabled";
  const snapshot = catalogSnapshot({ assets: [first, other] });
  snapshot.inventory.declarations[0].evidence.facts = { secret: "must-not-be-searchable" };
  const context = fresh(t, snapshot); const props = viewProps(snapshot);
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  assert.deepEqual(view.rows.value.map((asset) => asset.id), ["global-asset", "separate-identity"]);
  view.state.value = "enabled";
  assert.deepEqual(view.rows.value.map((asset) => asset.id), ["separate-identity"]);
  view.state.value = "all";
  for (const query of ["Codex CLI", "shared-tool", "/fixture/codex/config.json", "fixture-server"]) {
    props.query = query; assert.equal(view.rows.value.length, 2, query);
  }
  props.query = "must-not-be-searchable"; assert.equal(view.rows.value.length, 0);
  props.query = ""; view.source.value = "source-codex";
  props.catalog = { ...snapshot, inventory: { ...snapshot.inventory, sources: [] } };
  assert.equal(view.source.value, "");
  assert.equal(view.rows.value.length, 2);
});

test("incomplete discovery is visible without rows and stays bound to its Agent and category", async (t) => {
  const snapshot = catalogSnapshot({ assets: [] });
  snapshot.inventory.diagnostics = [
    { kind: "discoveryIncomplete", agentKind: "gemini", category: "skill", reason: "unsupportedVersion" },
    { kind: "discoveryIncomplete", agentKind: "claudeCode", category: "mcp", reason: "nativeEquivalenceUnobserved" },
  ];
  const { context, props, view } = provenanceView(t, snapshot);
  props.page = "skill";
  assert.equal(view.discoveryNotes.value.length, 1);
  assert.match(view.discoveryNotes.value[0], /gemini CLI.*当前版本/);
  for (const agent of [null, "gemini"] as const) {
    props.agentFilter = agent;
    for (const query of ["", " \t "]) {
      props.query = query;
      const html = await renderSurface("/src/components/agent-workspace/AgentCatalogPanel.vue", { ...props, loading: false, error: "", busy: () => false }, context.pinia);
      assert.match(html, /扫描说明/);
      assert.match(html, agent ? /没有匹配的 Skill/ : /暂未读取到内容/);
    }
  }
  props.query = "missing-asset";
  const filtered = await renderSurface("/src/components/agent-workspace/AgentCatalogPanel.vue", { ...props, loading: false, error: "", busy: () => false }, context.pinia);
  assert.match(filtered, /没有匹配的 Skill/);
  assert.match(filtered, /扫描说明/); assert.match(filtered, /当前版本/);
  assert.doesNotMatch(filtered, /暂未读取到内容/);
  props.query = "";
  props.agentFilter = "claudeCode";
  assert.deepEqual(view.discoveryNotes.value, []);
  props.page = "mcp";
  assert.equal(view.discoveryNotes.value.length, 1);
  assert.match(view.discoveryNotes.value[0], /Claude CLI.*去重结果/);
});

test("asset refinements remain distinct from Agent context and clearing them preserves discovery notices", (t) => {
  const snapshot = catalogSnapshot({ assets: [] });
  snapshot.inventory.diagnostics = [{ kind: "discoveryIncomplete", agentKind: "codex", category: "mcp", reason: "unsupportedVersion" }];
  const context = fresh(t, snapshot); const props = viewProps(snapshot); props.agentFilter = "codex";
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  const notes = view.discoveryNotes.value;
  assert.equal(notes.length, 1); assert.equal(view.hasFilters.value, true); assert.equal(view.hasAssetFilters.value, false);
  for (const refine of [
    () => { view.state.value = "enabled"; },
    () => { view.source.value = "source-codex"; },
    () => { view.provision.value = "pluginProvided"; },
    () => { view.installation.value = "nativePackage"; },
    () => { view.scope.value = "managed"; },
  ]) {
    refine();
    assert.equal(view.rows.value.length, 0); assert.equal(view.hasAssetFilters.value, true);
    assert.deepEqual(view.discoveryNotes.value, notes);
    view.clearFilters();
    assert.equal(props.agentFilter, "codex"); assert.equal(view.hasFilters.value, true);
    assert.equal(view.hasAssetFilters.value, false); assert.deepEqual(view.discoveryNotes.value, notes);
  }
});

test("mixed provenance keeps one identity and all facets must match one Agent binding", (t) => {
  const { props, view } = provenanceView(t);
  assert.equal(view.rows.value.length, 1); assert.equal(view.bindingsFor(view.rows.value[0]).length, 3);
  props.agentFilter = "codex"; view.provision.value = "pluginProvided";
  assert.equal(view.rows.value.length, 0);
  props.agentFilter = null; view.state.value = "enabled";
  assert.equal(view.rows.value.length, 0);
  view.state.value = "disabled"; view.source.value = "source-codex";
  assert.equal(view.rows.value.length, 0);
  props.agentFilter = "claudeCode"; view.source.value = "source-claudeCode";
  view.installation.value = "nativePackage"; view.scope.value = "user";
  props.query = "claudeCode-only-description";
  assert.equal(view.rows.value[0].id, "mixed-provenance");
  assert.deepEqual(view.bindingsFor(view.rows.value[0]).map((binding) => binding.native.agentKind), ["claudeCode"]);
  view.scope.value = "managed";
  assert.equal(view.rows.value.length, 0);
});

test("facets and search cannot borrow another provenance entry on the same native binding", (t) => {
  const snapshot = catalogProvenanceSnapshot(); const asset = snapshot.assets[0];
  const binding = asset.bindings[0]; asset.bindings = [binding];
  const shared = assetSource("shared-second", { scope: "user", origin: "sharedFiles", path: "/fixture/second/shared/SKILL.md" });
  snapshot.inventory.sources.push(shared);
  binding.native.sourceIds.push(shared.id); binding.native.scope = "user";
  binding.native.provenance.push({ sourceId: shared.id, declarationId: "second-definition", scope: "user",
    provision: "independent", installation: "sharedFiles", provider: "unknown" });
  asset.provenance = { provisions: ["agentBuiltIn", "independent"], installations: ["bundled", "sharedFiles"], providers: ["agentVendor", "unknown"] };
  const { props, view } = provenanceView(t, snapshot);
  view.provision.value = "agentBuiltIn"; view.installation.value = "sharedFiles";
  assert.equal(view.rows.value.length, 0);
  view.installation.value = "all"; view.scope.value = "user";
  assert.equal(view.rows.value.length, 0);
  view.scope.value = "all"; view.source.value = shared.id;
  assert.equal(view.rows.value.length, 0);
  view.source.value = "";
  for (const query of ["独立配置", "共享目录", "作者未确认", shared.path, "用户"]) {
    props.query = query; assert.equal(view.rows.value.length, 0, query);
  }
  props.query = "Agent 官方"; assert.equal(view.rows.value.length, 1);
  view.provision.value = "independent"; props.query = binding.native.path!;
  assert.equal(view.rows.value.length, 0, "the inspection path belongs to the other definition");
  props.query = shared.path; assert.equal(view.rows.value.length, 1);
});

test("search is limited to the surviving binding variant and provenance", (t) => {
  const { props, view } = provenanceView(t);
  props.agentFilter = "codex"; props.query = "claudeCode-only-description";
  assert.equal(view.rows.value.length, 0);
  props.agentFilter = null; view.state.value = "disabled"; props.query = "codex-only-description";
  assert.equal(view.rows.value.length, 0);
  view.state.value = "all"; view.provision.value = "pluginProvided"; props.query = "Agent 自带";
  assert.equal(view.rows.value.length, 0);
  view.clearFilters(); props.query = "gemini-only-description";
  assert.equal(view.rows.value.length, 1);
  assert.deepEqual(view.bindingsFor(view.rows.value[0]).map((binding) => binding.native.agentKind), ["gemini"]);
  props.query = "Review skill";
  assert.equal(view.bindingsFor(view.rows.value[0]).length, 3);
});

test("configuration scope uses logical provenance instead of the physical file or installation origin", (t) => {
  const binding = catalogBinding("claudeCode");
  binding.native.provenance[0].scope = "workspace";
  const snapshot = catalogSnapshot({ assets: [catalogAsset("project-mcp", { bindings: [binding] })] });
  snapshot.inventory.sources[0].scope = "user";
  const context = fresh(t, snapshot); const props = viewProps(snapshot);
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  view.scope.value = "workspace"; view.installation.value = "configEntry"; props.query = "工作区";
  assert.equal(view.rows.value.length, 1);
  props.query = ""; view.scope.value = "user";
  assert.equal(view.rows.value.length, 0);
  view.scope.value = "workspace"; view.installation.value = "sharedFiles";
  assert.equal(view.rows.value.length, 0);
});

test("refresh clears missing configuration paths, preserves the other selectors and clear resets them", (t) => {
  const snapshot = catalogProvenanceSnapshot();
  const overlay = assetSource("overlay-only"); snapshot.inventory.sources.push(overlay);
  snapshot.assets[0].bindings[0].native.sourceIds.push(overlay.id);
  const { props, view } = provenanceView(t, snapshot);
  assert.equal(view.sourceOptions.value.some((source) => source.id === overlay.id), false);
  view.source.value = overlay.id; assert.equal(view.rows.value.length, 0);
  props.agentFilter = "codex"; props.query = "Review skill";
  view.source.value = "source-codex"; view.provision.value = "agentBuiltIn"; view.installation.value = "bundled";
  view.scope.value = "system"; view.state.value = "enabled";
  props.catalog = { ...snapshot, inventory: { ...snapshot.inventory, sources: snapshot.inventory.sources.filter((source) => source.id !== "source-codex") } };
  assert.equal(view.source.value, ""); assert.equal(view.rows.value.length, 1);
  assert.equal(view.provision.value, "agentBuiltIn"); assert.equal(view.installation.value, "bundled");
  assert.equal(view.scope.value, "system"); assert.equal(view.state.value, "enabled");
  view.clearFilters();
  assert.deepEqual([view.state.value, view.provision.value, view.installation.value, view.scope.value], ["all", "all", "all", "all"]);
  assert.equal(view.source.value, ""); assert.equal(props.query, "Review skill");
  assert.equal(view.hasFilters.value, true);
  props.agentFilter = null; assert.equal(view.hasFilters.value, false);
});

test("unapplied shared definitions do not inherit native provenance from the library or unrelated sources", async (t) => {
  const asset = catalogAsset("unapplied", { bindings: [], provenance: { provisions: [], installations: [], providers: [] } });
  const snapshot = catalogSnapshot({ assets: [asset] }); snapshot.inventory.sources.push(assetSource("unrelated", { origin: "sharedFiles" }));
  const context = fresh(t, snapshot); const props = viewProps(snapshot);
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  props.query = "fixture-server"; assert.equal(view.rows.value.length, 1);
  view.provision.value = "independent"; assert.equal(view.rows.value.length, 0);
  view.provision.value = "unknown"; assert.equal(view.rows.value.length, 0);
  view.clearFilters(); view.installation.value = "sharedFiles"; assert.equal(view.rows.value.length, 0);
  view.clearFilters(); view.scope.value = "user"; assert.equal(view.rows.value.length, 0);
  view.clearFilters(); props.agentFilter = "codex"; assert.equal(view.rows.value.length, 0);
  props.agentFilter = null;
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogPanel.vue", { ...props, loading: false, error: "", busy: () => false }, context.pinia);
  assert.match(html, /<button[^>]*data-asset-feature="ownership-managed"[^>]*aria-label="共享库 v1"/);
  assert.match(html, /<button[^>]*data-asset-feature="unapplied"[^>]*aria-label="尚未配置到 Agent"/);
  assert.doesNotMatch(html, /data-asset-feature="(?:provision|installation|provider)-|class="agent-catalog-provenance"/);
});

test("catalog rows expose separate backend provenance icons and keep advanced filters collapsed", async (t) => {
  const { context, props } = provenanceView(t);
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogPanel.vue", { ...props, loading: false, error: "", busy: () => false }, context.pinia);
  assert.equal((html.match(/data-global-asset-id="mixed-provenance"/g) ?? []).length, 1);
  assert.equal((html.match(/class="agent-catalog-agent-control\b/g) ?? []).length, 3);
  const labels = [...html.matchAll(/<button[^>]*data-asset-feature="[^"]*"[^>]*aria-label="([^"]*)"/g)].map((match) => match[1]);
  assert.deepEqual(labels, ["共享库 v1", "Agent 自带", "插件提供", "随 Agent 内置", "原生扩展包", "共享目录", "3 份配置存在差异"]);
  assert.doesNotMatch(html, /class="agent-catalog-provenance"/);
  assert.match(html, /aria-label="筛选生效状态"/);
  assert.match(html, /aria-expanded="false"[^>]*aria-controls="agent-catalog-skill-more-filters"/);
  assert.doesNotMatch(html, /id="agent-catalog-skill-more-filters"/);
  const template = readFileSync(new URL("../src/components/agent-workspace/AgentCatalogPanel.vue", import.meta.url), "utf8");
  for (const label of ["提供方式", "安装来源", "配置范围", "配置路径"]) assert.ok(template.includes(`aria-label="筛选${label}"`), label);

});

test("catalog details preserve builtin and unknown author labels and use the parent's inspect capability", async (t) => {
  const snapshot = catalogProvenanceSnapshot(); const asset = snapshot.assets[0];
  const parent = assetRecord("parent-plugin", { agentKind: "claudeCode", category: "plugin", label: "Fixture review plugin",
    details: { kind: "plugin", installState: "installed", enabled: "enabled", trusted: "trusted" }, actions: [assetAction("inspect")] });
  asset.bindings[1].native.relationships.providedBy = parent.stableId;
  snapshot.inventory.assets.push(parent);
  const { context, props } = provenanceView(t, snapshot);
  const surface = { visible: true, asset, catalog: snapshot, agents: props.agents, libraryBusy: {}, libraryErrors: {} };
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogDetailDrawer.vue", surface, context.pinia);
  assert.match(html, /Agent 自带 · 随 Agent 内置 · Agent 官方/);
  assert.match(html, /插件提供 · 原生扩展包/); assert.doesNotMatch(html, /用户声明自制/);
  assert.match(html, /系统/); assert.doesNotMatch(html, /作者未确认/);
  for (const source of snapshot.inventory.sources) assert.ok(html.includes(source.path), source.path);
  const parentButton = html.match(/<button\b[^>]*>[\s\S]*?<\/button>/g)?.find((button) => button.includes("Fixture review plugin")) ?? "";
  assert.ok(parentButton); assert.doesNotMatch(parentButton, /disabled/);
  parent.actions = [assetAction("inspect", { available: false, reason: "sourceUnavailable" })];
  const denied = await renderSurface("/src/components/agent-workspace/AgentCatalogDetailDrawer.vue", surface, context.pinia);
  const deniedParentButton = denied.match(/<button\b[^>]*>[\s\S]*?<\/button>/g)?.find((button) => button.includes("Fixture review plugin")) ?? "";
  assert.ok(deniedParentButton);
});

test("native provenance details retain logical scope and source-only details show physical origin separately", async (t) => {
  const snapshot = catalogSnapshot(); const binding = snapshot.assets[0].bindings[0];
  binding.native.provenance[0].scope = "workspace";
  const context = fresh(t, snapshot);
  const { useAgentAssetCatalog } = await server.ssrLoadModule("/src/composables/useAgentAssetCatalog.ts") as typeof import("../src/composables/useAgentAssetCatalog.ts");
  const nativeCatalog = context.ui(() => useAgentAssetCatalog(ref(snapshot.inventory))).value;
  const surface = { visible: true, detail: nativeCatalog.detailFor(binding.native.stableId), source: null, sourceContext: null,
    sourceInstallations: [], preview: null, previewState: "idle", previewError: "", copiedPathId: null, busy: () => false, error: () => null };
  const path = "/src/components/settings/agent-environment/AgentAssetDetailDrawer.vue";
  const html = await renderSurface(path, surface, context.pinia);
  assert.match(html, /独立配置 · 配置文件注册/); assert.match(html, /配置范围：工作区/);
  assert.ok(html.includes(snapshot.inventory.sources[0].path)); assert.doesNotMatch(html, /用户声明自制/);
  const source = assetSource("shared-user-source", { scope: "user", origin: "sharedFiles" });
  const sourceHtml = await renderSurface(path, { ...surface, detail: null, source }, context.pinia);
  assert.match(sourceHtml, /<span>用户<\/span>/); assert.match(sourceHtml, /安装来源<strong>共享目录<\/strong>/);
});

test("missing application receipts remain visible under the Agent filter without inventing a native binding", async (t) => {
  const asset = catalogAsset("missing-shared-asset", { bindings: [], provenance: { provisions: [], installations: [], providers: [] }, unresolvedTargets: [
    { targetId: "target-codex", contextId: "context-codex", agentKind: "codex", scope: "user", drift: "missing", state: "missing", actions: [], appliedVersion: 2, message: "目标已删除" },
    { targetId: "target-claudeCode", contextId: "context-claudeCode", agentKind: "claudeCode", scope: "user", drift: "unknown", state: "unknown", actions: [], appliedVersion: 1, message: "盘点证据不完整" },
  ] });
  const snapshot = catalogSnapshot({ assets: [asset] }); const context = fresh(t, snapshot); const props = viewProps(snapshot);
  props.agentFilter = "codex";
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  assert.equal(view.rows.value[0].id, asset.id); assert.equal(view.bindingsFor(asset).length, 0);
  assert.equal(view.unresolvedFor(asset)[0].targetId, "target-codex");
  props.query = "盘点证据不完整"; assert.equal(view.rows.value.length, 0);
  props.query = "目标已删除"; assert.equal(view.rows.value.length, 1);
  view.provision.value = "unknown"; assert.equal(view.rows.value.length, 0);
  view.clearFilters(); props.query = "";
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogDetailDrawer.vue", { visible: true, asset, catalog: snapshot,
    agents: props.agents, libraryBusy: {}, libraryErrors: {} }, context.pinia);
  assert.match(html, /已应用 v2 · 当前缺失/); assert.match(html, /已应用 v1 · 状态未知/);
  assert.doesNotMatch(html, /重新应用到此目标/); assert.doesNotMatch(html, /原生详情与预览/);
});

test("Agent controls always offer management while native inspection stays separately capability gated", async (t) => {
  const asset = catalogAsset();
  asset.bindings[0].native.actions = [assetAction("inspect", { available: false, reason: "sourceUnavailable" })];
  const snapshot = catalogSnapshot({ assets: [asset] }); const context = fresh(t, snapshot);
  const { agentCatalogAgentControls } = await server.ssrLoadModule("/src/utils/agent-catalog-row.ts") as typeof import("../src/utils/agent-catalog-row.ts");
  const labels = new Map([["codex" as const, "Codex CLI"], ["claudeCode" as const, "Claude Code"]]);
  const before = agentCatalogAgentControls(asset, labels)[0];
  assert.equal("action" in before, false);
  asset.bindings[0].actions = [assetAction("enable", { available: false, reason: "sourceUnavailable" })];
  assert.deepEqual(agentCatalogAgentControls(asset, labels)[0], before, "status and entry intent do not depend on a guessed toggle permission");
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogPanel.vue", { ...viewProps(snapshot), loading: false, error: "", busy: () => false }, context.pinia);
  const control = html.match(/<button[^>]*class="agent-catalog-agent-control[^"]*"[^>]*data-agent-kind="codex"[^>]*>/)?.[0] ?? "";
  assert.ok(control); assert.doesNotMatch(control, /\sdisabled(?:=|\s|>)/);
  assert.match(control, /aria-haspopup="dialog"/); assert.match(control, /管理 Shared tool 在 Codex CLI 中的使用/);
  const detail = await renderSurface("/src/components/agent-workspace/AgentCatalogDetailDrawer.vue", { visible: true, asset, catalog: snapshot,
    agents: [workspaceAgent()], libraryBusy: {}, libraryErrors: {} }, context.pinia);
  assert.match(detail, /查看原生配置/);
});

test("overlapping catalog refreshes share one scan and release all callers on failure", async (t) => {
  const context = fresh(t);
  for (const fails of [false, true]) {
    const response = pending("get_agent_asset_catalog");
    const start = calls.filter(call => call.command === "get_agent_asset_catalog").length;
    const first = context.catalog.refresh(); const second = context.catalog.refresh();
    const latest = catalogSnapshot({ revision: `latest-${fails}` }); latest.inventory.scannedAt = `latest-scan-${fails}`;
    if (fails) response.reject(new Error("catalog read failed")); else response.resolve(latest);
    const [one, two] = await Promise.all([first, second]);
    assert.equal(one, two);
    assert.equal(calls.filter(call => call.command === "get_agent_asset_catalog").length, start + 1);
    assert.equal(context.catalog.loading.__native__, false);
    if (fails) {
      assert.equal(one, null); assert.match(context.catalog.loadErrors.__native__, /catalog read failed/);
      assert.equal(context.catalog.catalogs.__native__.revision, "latest-false");
    } else {
      assert.equal(context.catalog.catalogs.__native__.revision, latest.revision);
      assert.deepEqual(context.environment.inventories.__native__.assets, latest.inventory.assets);
      assert.equal(context.environment.inventoryState.__native__, "ready");
    }
  }
});

test("a late native inventory response cannot replace a newer global catalog publication", async (t) => {
  const context = fresh(t); const native = pending("get_agent_environment_inventory");
  const nativeRead = context.environment.loadInventory(undefined, true);
  await context.catalog.refresh(); const accepted = context.environment.inventories.__native__;
  native.resolve({ ...backendCatalog.inventory, scannedAt: "obsolete-native-scan" }); await nativeRead;
  assert.equal(context.environment.inventories.__native__.scannedAt, accepted.scannedAt);
  assert.deepEqual(context.environment.inventories.__native__.assets, context.catalog.catalogs.__native__.inventory.assets);
});

async function discoverCatalog(controller: ReturnType<CatalogConsoleModule["useAgentCatalogConsole"]>, plan = catalogPlan()) {
  const response = pending("plan_agent_catalog"); const opening = controller.openPlan(plan.assetId, plan.action);
  response.resolve({ ...plan, token: null, planId: null }); await opening;
}
async function selectCatalogTargets(controller: ReturnType<CatalogConsoleModule["useAgentCatalogConsole"]>, plan: AgentCatalogPlan) {
  controller.setTargets(plan.targets.filter((target) => target.available).map((target) => target.targetId));
  if (!controller.selectedTargets.value.length) return;
  const response = pending("plan_agent_catalog"); const preparing = controller.preparePlan(); response.resolve(plan); await preparing;
}
async function prepareCatalog(controller: ReturnType<CatalogConsoleModule["useAgentCatalogConsole"]>, plan = catalogPlan()) {
  await discoverCatalog(controller, plan);
  await selectCatalogTargets(controller, plan);
}

test("catalog confirmation closes before apply settles and failed submission releases local state", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const tasks = context.ui(() => backgrounds.useAgentBackgroundTasks()).value;
  await prepareCatalog(controller);
  assert.deepEqual(calls.filter((call) => call.command === "plan_agent_catalog").at(-1)?.args, { request: {
    source: { kind: "catalog", assetId: "global-asset", expectedVersion: 1 }, action: "applyDefinition", targetIds: ["target-codex"], expectedRevision: "catalog-revision", workspace: null,
  } });
  const apply = pending("apply_agent_catalog"); controller.confirmPlan();
  assert.equal(controller.planVisible.value, false); assert.equal(controller.pendingPlan.value, null);
  assert.equal(context.catalog.starting["global-asset"], true);
  assert.equal(tasks.value.find((task) => task.id === "agent-catalog-start-global-asset")?.status, "running");
  context.navigation.openPage("sessions", "claudeCode");
  assert.equal(context.navigation.page, "sessions"); assert.equal(controller.planVisible.value, false);
  apply.reject({ kind: "sourceConflict", message: "配置已经变化" }); await settle();
  assert.equal(context.catalog.starting["global-asset"], false);
  assert.match(context.catalog.startErrors["global-asset"], /配置已经变化/);
  assert.equal(tasks.value.find((task) => task.id === "agent-catalog-start-global-asset")?.status, "failed");
  assert.deepEqual(calls.find((call) => call.command === "apply_agent_catalog")?.args, { request: {
    planToken: "catalog-plan", assetId: "global-asset", action: "applyDefinition",
  } });
});

test("completed catalog application preserves a newer draft until its deferred refresh is safe", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  await prepareCatalog(controller);
  const response = pending("apply_agent_catalog"); controller.confirmPlan();
  await controller.openEditor(null, "skill");
  response.resolve(catalogOperation({ phase: "completed", canCancel: false, definitionChange: { kind: "save", state: "saved", version: 2, message: null },
    targets: [{ ...catalogOperation().targets[0], phase: "completed", outcome: "appliedVerified" }] }));
  await settle();
  assert.equal(controller.editorVisible.value, true); assert.equal(controller.editorAssetId.value, null);
  assert.equal(context.catalog.starting["global-asset"], false);
  assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 0);
  controller.closeEditor(); await settle();
  assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 1);
});

test("catalog plans keep unavailable targets and native binding identities under backend control", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const unavailable = { ...context.catalog.catalogs.__native__.targets[0], available: false, reason: "当前版本未验证分发" };
  context.catalog.catalogs = { __native__: catalogSnapshot({ assets: [catalogAsset(undefined, {
    application: { available: true, sourceBindingId: null, targets: [unavailable], observations: catalogObservations(["codex", "claudeCode"]), reason: null },
  })] }) };
  await prepareCatalog(controller, catalogPlan({ targets: [{ ...catalogPlan().targets[0], available: false, reason: unavailable.reason }] }));
  assert.equal(controller.targetChoices.value[0].available, false); assert.match(controller.targetChoices.value[0].reason, /未验证/);
  assert.equal(controller.canConfirm.value, false); controller.confirmPlan();
  assert.equal(calls.filter((call) => call.command === "apply_agent_catalog").length, 0);
  const nativePlan = catalogPlan({ action: "enable", targets: [{ ...catalogPlan().targets[0], targetId: "binding-codex" }] });
  await prepareCatalog(controller, nativePlan);
  assert.deepEqual((calls.filter((call) => call.command === "plan_agent_catalog").at(-1)?.args.request as { targetIds: string[] }).targetIds, ["binding-codex"]);
});

test("catalog preparation timeout releases busy state and late completion cannot restore the plan", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  await discoverCatalog(controller);
  controller.setTargets(["target-codex"]);
  await fastTimeout(15_000, async () => {
    const request = pending("plan_agent_catalog"); await controller.preparePlan();
    assert.equal(controller.preparing.value, false); assert.match(controller.planError.value, /超时/);
    controller.closePlan(); request.resolve(catalogPlan()); await settle();
    assert.equal(controller.planVisible.value, false); assert.equal(controller.pendingPlan.value, null);
  });
});

test("catalog apply timeout never retries the write and keeps navigation usable", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  await prepareCatalog(controller);
  await fastTimeout(15_000, async () => {
    const apply = pending("apply_agent_catalog"); controller.confirmPlan();
    context.navigation.setView("agents"); context.navigation.setView("providers");
    await until(() => !context.catalog.starting["global-asset"]);
    assert.equal(controller.planVisible.value, false); assert.match(context.catalog.startErrors["global-asset"], /不会自动重试/);
    apply.resolve(catalogOperation()); await settle();
    assert.equal(Object.keys(context.catalog.operations).length, 0);
    assert.equal(calls.filter((call) => call.command === "apply_agent_catalog").length, 1);
  });
});

test("navigation discards editor reads while metadata refresh preserves drafts and invalidates changed plans", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const definition = pending("get_agent_catalog_definition");
  const reading = controller.openEditor("global-asset", "skill");
  context.navigation.openPage("sessions");
  definition.resolve(catalogDefinition()); await reading;
  assert.equal(controller.editorVisible.value, false);
  assert.equal(controller.editorDefinition.value, null);
  const nextDefinition = pending("get_agent_catalog_definition");
  const nextReading = controller.openEditor("global-asset", "skill");
  nextDefinition.resolve(catalogDefinition()); await nextReading;
  await context.catalog.refresh();
  assert.equal(controller.editorVisible.value, true);
  assert.equal(controller.editorDefinition.value?.name, catalogDefinition().name);
  await prepareCatalog(controller);
  assert.ok(controller.pendingPlan.value);
  backendCatalog = catalogSnapshot({ revision: "changed-revision" });
  await context.catalog.refresh();
  assert.equal(controller.pendingPlan.value, null);
  assert.equal(controller.planVisible.value, true);
});

test("failed edit fetches cannot turn into creation and closing adoption does not reopen old detail", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const response = pending("get_agent_catalog_definition"); const reading = controller.openEditor("global-asset", "skill");
  response.reject(new Error("共享定义不可读取")); await reading;
  assert.equal(controller.editorLoading.value, false); assert.equal(controller.editorAssetId.value, "global-asset");
  await controller.saveDefinition({ assetId: null, expectedVersion: null, name: "accidental duplicate", category: "skill", mcp: null, hook: null, skillMarkdown: "# fixture" });
  assert.equal(calls.filter((call) => call.command === "save_agent_catalog_definition").length, 0);
  context.catalog.catalogs.__native__.assets[0].bindings[0].canAdopt = true;
  controller.closeEditor(); controller.openDetail("global-asset");
  const adoption = pending("adopt_agent_catalog_asset"); const adopting = controller.adoptBinding("binding-codex");
  controller.closeDetail(); adoption.resolve(catalogDefinition()); await adopting;
  assert.equal(controller.detailVisible.value, false); assert.equal(controller.libraryBusy.value["binding-codex"], false);
});

function portableSkillSnapshot(version: number | null = null, targets: AgentCatalogTarget[] = [catalogTarget("claudeCode")]) {
  const bindings = [catalogBinding("codex", "not-the-selected-source"), catalogBinding("codex", "backend-selected-source")];
  for (const binding of bindings) { binding.native.category = "skill"; binding.canAdopt = true; }
  return catalogSnapshot({ revision: `portable-revision-${version ?? "observed"}`, assets: [catalogAsset("portable-skill", {
    name: "便携审阅", category: "skill", ownership: version === null ? "observed" : "managed", version, bindings,
    application: { available: true, sourceBindingId: version === null ? "backend-selected-source" : null, targets, observations: catalogObservations(["codex"]), reason: null },
  })] });
}

function portablePlan(targets: AgentCatalogTarget[] = [catalogTarget("claudeCode")], fields: Partial<AgentCatalogPlan> = {}) {
  return catalogPlan({ assetId: "portable-skill", version: null,
    definitionChange: { kind: "adopt", name: "便携审阅", beforeVersion: null, afterVersion: 1 },
    targets: targets.map((target) => ({ targetId: target.id, contextId: target.contextId, agentKind: target.agentKind, scope: target.scope,
      targetKind: "destination", label: target.label, available: target.available, reason: target.reason, affectedAssetIds: [],
      changes: [{ label: "应用 Skill", path: "/fixture/" + target.id + "/SKILL.md", before: null, after: "# 审阅" }] })), ...fields });
}
function draftRequest(fields: Partial<AgentCatalogSaveRequest> = {}): AgentCatalogSaveRequest {
  return { assetId: null, expectedVersion: null, name: "新审阅规则", category: "skill", mcp: null, hook: null, skillMarkdown: "# 审阅\n保留用户配置。", ...fields };
}
function catalogWrites() {
  return calls.filter((call) => ["adopt_agent_catalog_asset", "save_agent_catalog_definition", "apply_agent_catalog", "commit_agent_catalog_relation"].includes(call.command));
}

test("observed Skill discovery and selected preview use the exact native source without adoption or native writes", async (t) => {
  const targets = [catalogTarget("claudeCode"), catalogTarget("claudeCode", { id: "claude-project", contextId: "claude-workspace", scope: "workspace" }), catalogTarget("gemini")];
  const snapshot = portableSkillSnapshot(); const context = fresh(t, snapshot); const { value: controller } = catalogUi(context);
  const discovery = pending("plan_agent_catalog"); const opening = controller.openPlan("portable-skill", "applyDefinition");
  assert.equal(controller.planVisible.value, true); assert.equal(controller.preparing.value, true);
  assert.deepEqual(controller.targetChoices.value, []); assert.deepEqual(catalogWrites(), []);
  discovery.resolve({ ...portablePlan(targets), token: null, planId: null }); await opening;
  assert.deepEqual(controller.targetChoices.value.map((target) => target.id), targets.map((target) => target.id));
  assert.deepEqual(controller.selectedTargets.value, []); assert.equal(controller.canConfirm.value, false);
  const selected = portablePlan([targets[1]]);
  await selectCatalogTargets(controller, selected);
  const requests = calls.filter((call) => call.command === "plan_agent_catalog").map((call) => call.args.request as AgentCatalogPlanRequest);
  assert.deepEqual(requests.map((request) => request.targetIds), [[], ["claude-project"]]);
  for (const request of requests) assert.deepEqual(request.source, { kind: "nativeBinding", assetId: "portable-skill", bindingId: "backend-selected-source" });
  assert.equal(requests[1].expectedRevision, snapshot.revision);
  assert.equal(controller.canConfirm.value, true); assert.deepEqual(catalogWrites(), []);
  controller.closePlan();
  assert.equal(controller.planVisible.value, false); assert.deepEqual(catalogWrites(), []);
  assert.equal(context.catalog.catalogs.__native__.assets[0].ownership, "observed");
});

test("an ambiguous application source stays inspectable without choosing another binding or target", async (t) => {
  const snapshot = portableSkillSnapshot();
  snapshot.assets[0].application = { ...snapshot.assets[0].application, available: false, sourceBindingId: null, reason: "多个来源配置存在差异" };
  const context = fresh(t, snapshot); const { value: controller } = catalogUi(context);
  await controller.openPlan("portable-skill", "applyDefinition");
  assert.equal(controller.detailVisible.value, true); assert.equal(controller.planVisible.value, false);
  assert.equal(controller.selectedAsset.value?.application.reason, "多个来源配置存在差异");
  assert.equal(calls.length, 0);
});

for (const editing of [false, true]) {
  test((editing ? "editing" : "creating") + " save-and-apply keeps a draft through explicit target preview and cancellation makes zero writes", async (t) => {
    const context = fresh(t); const { value: controller } = catalogUi(context);
    const request = draftRequest(editing ? { assetId: "global-asset", expectedVersion: 1 } : {});
    if (editing) {
      const definition = pending("get_agent_catalog_definition"); const opening = controller.openEditor("global-asset", "skill");
      definition.resolve(catalogDefinition()); await opening;
    } else await controller.openEditor(null, "skill");
    const plan = catalogPlan({ assetId: editing ? "global-asset" : "new-draft-asset",
      definitionChange: { kind: "save", name: request.name, beforeVersion: request.expectedVersion, afterVersion: editing ? 2 : 1 } });
    const discovery = pending("plan_agent_catalog"); const opening = controller.saveAndApply(request);
    assert.equal(controller.editorVisible.value, false); assert.equal(controller.planVisible.value, true);
    assert.equal(controller.planName.value, request.name); assert.equal(controller.preparing.value, true);
    discovery.resolve({ ...plan, token: null, planId: null }); await opening;
    await selectCatalogTargets(controller, plan);
    for (const call of calls.filter((call) => call.command === "plan_agent_catalog")) {
      assert.deepEqual((call.args.request as AgentCatalogPlanRequest).source, { kind: "draft", definition: request });
    }
    assert.equal(controller.canConfirm.value, true); assert.deepEqual(catalogWrites(), []);
    controller.closePlan();
    assert.equal(controller.pendingPlan.value, null); assert.equal(controller.planName.value, ""); assert.deepEqual(catalogWrites(), []);
    assert.equal(context.catalog.catalogs.__native__.assets[0].version, 1);
  });
}

for (const event of ["close", "navigation", "refresh"] as const) {
  test("native preview " + event + " preserves refresh continuity or cancels dismissed reads without writes", async (t) => {
    const context = fresh(t, portableSkillSnapshot()); const { value: controller } = catalogUi(context);
    const response = pending("plan_agent_catalog"); const opening = controller.openPlan("portable-skill", "applyDefinition");
    if (event === "close") controller.closePlan();
    else if (event === "navigation") context.navigation.openPage("sessions");
    else await context.catalog.refresh();
    assert.equal(controller.preparing.value, event === "refresh"); assert.equal(controller.planVisible.value, event === "refresh");
    response.resolve({ ...portablePlan(), token: null, planId: null }); await opening;
    assert.equal(controller.pendingPlan.value, null); assert.equal(controller.planError.value, ""); assert.deepEqual(catalogWrites(), []);
  });
}

test("a newer draft preview rejects the old discovery result and a read timeout stays retryable", async (t) => {
  const context = fresh(t, portableSkillSnapshot()); const { value: controller } = catalogUi(context);
  const old = pending("plan_agent_catalog"); const oldOpening = controller.openPlan("portable-skill", "applyDefinition");
  await until(() => calls.some(call => call.command === "plan_agent_catalog"));
  await controller.openEditor(null, "skill");
  const current = pending("plan_agent_catalog"); const draft = draftRequest();
  const opening = controller.saveAndApply(draft);
  current.resolve(catalogPlan({ assetId: "draft-current", token: null, planId: null })); await opening;
  old.resolve({ ...portablePlan(), token: null, planId: null }); await oldOpening;
  assert.equal(controller.planName.value, draft.name); assert.equal(controller.pendingPlan.value, null, "candidate discovery is not a prepared plan");
  assert.deepEqual(controller.selectedTargets.value, []);
  assert.equal(controller.targetChoices.value.length, 1);
  controller.closePlan();
  await fastTimeout(15_000, async () => {
    const late = pending("plan_agent_catalog"); await controller.openPlan("portable-skill", "applyDefinition");
    assert.equal(controller.preparing.value, false); assert.equal(controller.planVisible.value, true); assert.match(controller.planError.value, /超时/);
    controller.closePlan(); late.resolve(portablePlan()); await settle();
    assert.equal(controller.pendingPlan.value, null); assert.deepEqual(catalogWrites(), []);
  });
});

test("Agent panels are loaded on demand and rapid Agent changes reject the old result", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  assert.equal(calls.length, 0);
  const older = pending("get_agent_catalog_agent_panel"); const first = controller.openAgentPanel("global-asset", "codex");
  await until(() => calls.some(call => call.command === "get_agent_catalog_agent_panel"));
  assert.equal(controller.management.loading.value, true);
  const newer = pending("get_agent_catalog_agent_panel"); const second = controller.openAgentPanel("global-asset", "claudeCode");
  newer.resolve(catalogAgentPanel("claudeCode")); await second;
  older.resolve(catalogAgentPanel()); await first;
  assert.deepEqual(controller.management.selection.value, { assetId: "global-asset", agentKind: "claudeCode" });
  assert.equal(controller.management.panel.value?.agentKind, "claudeCode"); assert.equal(controller.management.loading.value, false);
  assert.deepEqual(calls.find((call) => call.command === "get_agent_catalog_agent_panel")?.args, { request: {
    assetId: "global-asset", agentKind: "codex", expectedRevision: "catalog-revision", workspace: null,
  } });
  assert.deepEqual(catalogWrites(), []);
});

for (const event of ["close", "navigation", "refresh"] as const) {
  test("Agent panel " + event + " remains immediate while its IPC response is pending", async (t) => {
    const context = fresh(t); const { value: controller } = catalogUi(context);
    const response = pending("get_agent_catalog_agent_panel"); const opening = controller.openAgentPanel("global-asset", "codex");
    if (event === "close") controller.management.close();
    else if (event === "navigation") context.navigation.openPage("sessions");
    else await context.catalog.refresh();
    assert.equal(controller.management.loading.value, event === "refresh");
    assert.equal(controller.management.selection.value !== null, event === "refresh");
    response.resolve(catalogAgentPanel()); await opening;
    assert.equal(controller.management.panel.value !== null, event === "refresh"); assert.equal(controller.management.error.value, ""); assert.deepEqual(catalogWrites(), []);
  });
}

test("Agent panel timeout releases loading and an explicit retry preserves unknown evidence", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  await fastTimeout(15_000, async () => {
    const late = pending("get_agent_catalog_agent_panel"); await controller.openAgentPanel("global-asset", "claudeCode");
    assert.equal(controller.management.loading.value, false); assert.match(controller.management.error.value, /超时/);
    const retry = pending("get_agent_catalog_agent_panel"); const reading = controller.management.retry();
    const unknown = catalogAgentPanel("claudeCode", { entries: [], observation: { agentKind: "claudeCode", state: "unknown", reason: "YAML frontmatter 无法读取" } });
    retry.resolve(unknown); await reading;
    late.resolve(catalogAgentPanel("claudeCode")); await settle();
    assert.deepEqual(controller.management.panel.value?.observation, unknown.observation); assert.equal(controller.management.error.value, "");
    controller.management.close(); assert.deepEqual(catalogWrites(), []);
  });
});

const comparisonIntent = { kind: "compare", leftAssetId: "global-asset", rightAssetId: "other-asset" } as const;
async function openComparison(controller: ReturnType<CatalogConsoleModule["useAgentCatalogConsole"]>, preview = catalogRelationPreview()) {
  const response = pending("preview_agent_catalog_relation"); const opening = controller.relations.open(comparisonIntent);
  response.resolve(preview); await opening;
}
function relationMutationPlan(action: AgentCatalogRelationPreview["action"]) {
  return catalogRelationPreview({ action, token: "relation-token", expiresAt: new Date(Date.now() + 60_000).toISOString(), affectedBindingIds: ["binding-other"] });
}

test("comparison and keep-apart dismissal make no relation commit and unsupported directions retain backend reasons", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const preview = catalogRelationPreview(); await openComparison(controller, preview);
  assert.equal(controller.relations.canConfirm.value, false);
  await controller.relations.choose(preview.capabilities[1]);
  assert.equal(calls.filter((call) => call.command === "preview_agent_catalog_relation").length, 1);
  assert.match(controller.relations.preview.value?.capabilities[1].reason ?? "", /独立共享版本/);
  controller.relations.confirm(); controller.relations.close();
  assert.deepEqual(catalogWrites(), []); assert.equal(controller.relations.visible.value, false);
  const html = await renderSurface("/src/components/agent-workspace/AgentCatalogRelationModal.vue", { visible: true, preview, intent: comparisonIntent,
    agents: [workspaceAgent()], loading: false, error: "", expired: false, canConfirm: false, canBack: false }, context.pinia);
  assert.match(html, /关闭比较/); assert.match(html, /不再提醒/); assert.match(html, /独立共享版本历史不能合并/);
  assert.match(html, /文本差异/);
});

test("do-not-remind previews the exact backend intent then closes before a token-only relation commit", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  await openComparison(controller);
  const capability = controller.relations.preview.value!.capabilities[2];
  const response = pending("preview_agent_catalog_relation"); const choosing = controller.relations.choose(capability);
  response.resolve(relationMutationPlan("keepSeparate")); await choosing;
  assert.deepEqual(calls.filter((call) => call.command === "preview_agent_catalog_relation").at(-1)?.args, { request: {
    intent: capability.intent, expectedRevision: "catalog-revision", workspace: null,
  } });
  assert.deepEqual(catalogWrites(), []); assert.equal(controller.relations.canConfirm.value, true);
  const commit = pending("commit_agent_catalog_relation"); controller.relations.confirm();
  assert.equal(controller.relations.visible.value, false); assert.equal(Object.values(context.catalog.relationTasks)[0]?.state, "running");
  assert.deepEqual(calls.find((call) => call.command === "commit_agent_catalog_relation")?.args, { request: {
    planToken: "relation-token", relationKey: "relation-key", action: "keepSeparate",
  } });
  context.navigation.openPage("sessions");
  await controller.openEditor(null, "skill");
  commit.resolve({ assetIds: ["global-asset", "other-asset"], associationId: null, message: "已保留分开并隐藏提示" }); await settle();
  assert.equal(Object.values(context.catalog.relationTasks)[0]?.state, "completed");
  assert.equal(controller.editorVisible.value, true, "late relation refresh preserves a newer draft");
  assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 0);
  controller.closeEditor(); await settle();
  assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 1);
  assert.equal(catalogWrites().length, 1);
});

for (const intent of [{ kind: "restoreHint", leftAssetId: "global-asset", rightAssetId: "other-asset" }, { kind: "detach", associationId: "manual-association" }] satisfies AgentCatalogRelationIntent[]) {
  test(intent.kind + " uses a fresh backend preview and rejects unavailable or expired confirmation", async (t) => {
    const context = fresh(t); const { value: controller } = catalogUi(context);
    const response = pending("preview_agent_catalog_relation"); const opening = controller.relations.open(intent);
    response.resolve({ ...relationMutationPlan(intent.kind), available: false, reason: "当前来源不能完整验证" }); await opening;
    assert.deepEqual((calls.at(-1)?.args.request as { intent: unknown }).intent, intent);
    assert.equal(controller.relations.canConfirm.value, false); controller.relations.confirm();
    const retry = pending("preview_agent_catalog_relation"); const retrying = controller.relations.retry();
    retry.resolve({ ...relationMutationPlan(intent.kind), expiresAt: "invalid-date" }); await retrying;
    assert.equal(controller.relations.expired.value, true); assert.equal(controller.relations.canConfirm.value, false);
    controller.relations.confirm(); controller.relations.close(); assert.deepEqual(catalogWrites(), []);
  });
}

test("comparison navigation and newer previews reject late results without writes", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const old = pending("preview_agent_catalog_relation"); const oldOpening = controller.relations.open(comparisonIntent);
  await until(() => calls.some(call => call.command === "preview_agent_catalog_relation"));
  context.navigation.openPage("skill"); assert.equal(controller.relations.loading.value, false);
  await openComparison(controller, catalogRelationPreview({ relationKey: "current-relation" }));
  old.resolve(catalogRelationPreview({ relationKey: "old-relation" })); await oldOpening;
  assert.equal(controller.relations.preview.value?.relationKey, "current-relation");
  controller.relations.close(); assert.deepEqual(catalogWrites(), []);
});

test("relation commit timeout remains uncertain and only read-only refresh is offered", async (t) => {
  const context = fresh(t); const { value: controller } = catalogUi(context);
  const preview = pending("preview_agent_catalog_relation"); const opening = controller.relations.open({ kind: "detach", associationId: "manual-association" });
  preview.resolve(relationMutationPlan("detach")); await opening;
  await fastTimeout(30_000, async () => {
    const commit = pending("commit_agent_catalog_relation"); controller.relations.confirm();
    await until(() => Object.values(context.catalog.relationTasks)[0]?.state === "unknown");
    const task = Object.values(context.catalog.relationTasks)[0]; assert.match(task.message, /尚未确认.*不会自动重试/);
    await context.catalog.refreshRelation(task.id);
    commit.resolve({ assetIds: [], associationId: null, message: "迟到结果" }); await settle();
    assert.equal(context.catalog.relationTasks[task.id].state, "unknown");
    assert.equal(calls.filter((call) => call.command === "commit_agent_catalog_relation").length, 1);
    assert.equal(controller.relations.visible.value, false);
  });
});

test("a new draft lost apply response recovers by exact planId with its original workspace and name", async (t) => {
  const context = fresh(t); context.navigation.selectWorkspace("/fixture/original-project");
  context.catalog.catalogs = { "/fixture/original-project": catalogSnapshot() };
  const { value: controller } = catalogUi(context); const tasks = context.ui(() => backgrounds.useAgentBackgroundTasks()).value;
  await controller.openEditor(null, "skill");
  const request = draftRequest({ name: "只属于此次提交的草稿" });
  const plan = catalogPlan({ assetId: "new-draft-only", planId: "exact-new-draft-plan", definitionChange: { kind: "save", name: request.name, beforeVersion: null, afterVersion: 1 } });
  const discovery = pending("plan_agent_catalog"); const opening = controller.saveAndApply(request);
  discovery.resolve({ ...plan, token: null, planId: null }); await opening; await selectCatalogTargets(controller, plan);
  await fastTimeout(15_000, async () => {
    const lost = pending("apply_agent_catalog"); controller.confirmPlan();
    context.navigation.selectWorkspace("/fixture/other-project");
    await until(() => !context.catalog.starting[plan.assetId]);
    assert.match(context.catalog.startErrors[plan.assetId], /不会自动重试/);
    const failedSurface = await renderSurface("/src/components/agent-workspace/AgentOperationPanel.vue", { selectedPaths: {}, savingPath: null, pathErrors: {} }, context.pinia);
    assert.match(failedSurface, /未找到此任务，请返回任务中心刷新/);
    const operation = catalogOperation({ id: "recovered-draft-operation", planId: plan.planId!, assetId: plan.assetId, phase: "completed", canCancel: false,
      definitionChange: { kind: "save", state: "saved", version: 1, message: null },
      targets: [{ ...catalogOperation().targets[0], phase: "completed", outcome: "appliedVerified", message: "已应用", nativeOperationId: null }] });
    const recovery = pending("list_agent_catalog_operations"); const recovering = context.catalog.recover(); recovery.resolve([operation]); await recovering; await settle();
    assert.equal(context.catalog.operations[operation.id]?.planId, plan.planId); assert.equal(context.catalog.operationNames[operation.id], request.name);
    assert.equal(context.catalog.startErrors[plan.assetId], undefined); assert.equal(context.catalog.startTimes[plan.assetId], undefined);
    assert.equal(tasks.value.some((task) => task.id === "agent-catalog-start-" + plan.assetId), false);
    assert.match(tasks.value.find((task) => task.id === "agent-catalog-" + operation.id)?.title ?? "", /只属于此次提交的草稿/);
    assert.deepEqual(calls.filter((call) => call.command === "get_agent_asset_catalog").at(-1)?.args, { workspace: "/fixture/original-project" });
    lost.resolve(operation); await settle();
    assert.equal(calls.filter((call) => call.command === "apply_agent_catalog").length, 1);
    assert.equal(Object.keys(context.catalog.operations).length, 1);
  });
});

test("recovering an older plan cannot erase a newer submission error for the same asset", async (t) => {
  const context = fresh(t);
  const firstPlan = catalogPlan({ planId: "first-plan" }); const first = pending("apply_agent_catalog");
  const firstApplying = context.catalog.apply(firstPlan);
  first.reject(new Error("first submission unknown")); await firstApplying; await settle();
  const secondPlan = catalogPlan({ planId: "second-plan", token: "second-token" }); const second = pending("apply_agent_catalog");
  const secondApplying = context.catalog.apply(secondPlan);
  context.catalog.track(catalogOperation({ id: "old-operation", planId: "first-plan", phase: "completed", canCancel: false }));
  assert.equal(context.catalog.starting["global-asset"], true, "an older plan must not release the current submission");
  second.reject(new Error("new submission failure")); await secondApplying; await settle();
  const startedAt = context.catalog.startTimes["global-asset"];
  context.catalog.track(catalogOperation({ id: "old-operation", planId: "first-plan", phase: "completed", canCancel: false }));
  assert.match(context.catalog.startErrors["global-asset"], /new submission failure/);
  assert.equal(context.catalog.startTimes["global-asset"], startedAt);
  assert.equal(context.catalog.operationNames["old-operation"], "Shared tool");
  context.catalog.track(catalogOperation({ id: "new-operation", planId: "second-plan", phase: "completed", canCancel: false }));
  assert.equal(context.catalog.startErrors["global-asset"], undefined); assert.equal(context.catalog.startTimes["global-asset"], undefined);
});

test("catalog name order is stable across refreshes while an explicitly focused binding remains first", (t) => {
  const assets = [catalogAsset("z-id", { name: "Review 10", bindings: [catalogBinding("codex", "last-by-name")] }),
    catalogAsset("b-id", { name: "Review 2", bindings: [catalogBinding("codex", "second-by-name")] }),
    catalogAsset("a-id", { name: "Review 2", bindings: [catalogBinding("codex", "focused-binding")] })];
  const snapshot = catalogSnapshot({ assets }); const context = fresh(t, snapshot);
  const props = reactive({ ...viewProps(snapshot), focusedNativeId: null as string | null });
  const view = context.ui(() => catalogViews.useAgentCatalogView(props)).value;
  assert.deepEqual(view.rows.value.map((asset) => asset.id), ["a-id", "b-id", "z-id"]);
  props.catalog = catalogSnapshot({ assets: [...assets].reverse() });
  assert.deepEqual(view.rows.value.map((asset) => asset.id), ["a-id", "b-id", "z-id"]);
  props.focusedNativeId = assets[0].bindings[0].native.stableId;
  assert.equal(view.rows.value[0].id, "a-id", "focus does not reorder the list");
  assert.equal(view.isFocused(assets[0]), true);
  props.focusedNativeId = "native-focused-binding";
  assert.equal(view.rows.value[0].id, "a-id");
});

test("editable Skill Markdown keeps secret markers and leaves bundled resources with the definition", (t) => {
  const context = fresh(t); const definition = catalogDefinition(); const saved: AgentCatalogSaveRequest[] = [];
  const props = reactive({ visible: true, definition, editingAssetId: definition.assetId, category: definition.category, loading: false, saving: false });
  const editor = context.ui(() => editors.useAgentDefinitionEditor(props, (request) => saved.push(request))).value;
  assert.equal(editor.draft.skillMarkdown, definition.skillMarkdown);
  editor.draft.skillMarkdown += "\nAdditional instruction.\n"; editor.save();
  assert.equal(saved.length, 1);
  assert.match(saved[0].skillMarkdown!, /Additional instruction/);
  assert.equal(saved[0].assetId, definition.assetId); assert.equal(saved[0].expectedVersion, 1);
  assert.equal(definition.files[1].path, "scripts/check.sh"); assert.equal("files" in saved[0], false);
});

test("editor submission remains disabled while an existing definition is unavailable", (t) => {
  const context = fresh(t); const saved: AgentCatalogSaveRequest[] = [];
  const props = reactive({ visible: true, definition: null, editingAssetId: "global-asset", category: "skill" as const, loading: false, saving: false });
  const editor = context.ui(() => editors.useAgentDefinitionEditor(props, (request) => saved.push(request))).value;
  editor.draft.name = "Should not create"; editor.draft.skillMarkdown = "# fixture"; editor.save();
  assert.deepEqual(saved, []);
});

test("Hook removal background task renders removeBinding instead of disable", async (t) => {
  const context = fresh(t);
  context.catalog.operations = { "remove-hook-binding": catalogOperation({
    id: "remove-hook-binding", assetId: "global-hook", action: "removeBinding", phase: "completed", canCancel: false,
    targets: [{ targetId: "binding-claudeCode", label: "Claude Code 用户 Hook", phase: "completed",
      outcome: "appliedVerified", message: null, nativeOperationId: null }],
  }) };
  const html = await renderSurface("/src/components/agent-workspace/AgentOperationPanel.vue", {
    selectedPaths: {}, savingPath: null, pathErrors: {},
  }, context.pinia);
  assert.match(html, /<strong>移除配置<\/strong>/);
  assert.match(html, /Claude Code 用户 Hook/);
  assert.doesNotMatch(html, /停用/);
});

test("partial batch results remain failed and catalog-owned native operations are not duplicated", async (t) => {
  const context = fresh(t); const tasks = context.ui(() => backgrounds.useAgentBackgroundTasks()).value;
  context.environment.operations["native-child"] = assetOperation("native-child", { phase: "completed", canCancel: false, outcome: "appliedVerified" });
  context.environment.operations.standalone = assetOperation("standalone", { phase: "completed", canCancel: false, outcome: "appliedVerified" });
  context.catalog.track(catalogOperation({ phase: "completed", revision: 2, canCancel: false,
    definitionChange: { kind: "save", state: "saved", version: 2, message: "定义文件已写入" }, targets: [
    { targetId: "target-codex", label: "Codex", phase: "completed", outcome: "appliedVerified", message: null, nativeOperationId: "native-child" },
    { targetId: "target-claudeCode", label: "Claude", phase: "completed", outcome: "unchangedFailure", message: "fixture failure", nativeOperationId: null },
  ] }));
  await settle();
  const batch = tasks.value.find((task) => task.id === "agent-catalog-catalog-operation")!;
  assert.equal(batch.status, "failed"); assert.match(batch.detail, /1\/2/);
  assert.match(batch.detail, /共享版本 v2 已保存/); assert.match(batch.detail, /定义文件已写入/);
  assert.match(batch.detail, /Codex：/); assert.match(batch.detail, /Claude：.*fixture failure/);
  const html = await renderSurface("/src/components/agent-workspace/AgentOperationPanel.vue", { selectedPaths: {}, savingPath: null, pathErrors: {} }, context.pinia);
  assert.match(html, /共享版本 v2 已保存/); assert.match(html, /fixture failure/);
  assert.equal(tasks.value.some((task) => task.id === "agent-native-native-child"), false);
  assert.equal(tasks.value.some((task) => task.id === "agent-native-standalone"), true);
  assert.deepEqual([...context.catalog.nativeOperationIds], ["native-child"]);
});

async function prepareLifecycle(controller: ReturnType<LifecycleConsoleModule["useAgentLifecycleConsole"]>, target = lifecycleTarget(), plan = lifecyclePlan()) {
  backendLifecycle = lifecycleStores.useAgentLifecycleStore().catalog ?? lifecycleCatalog([target]);
  controller.open(target.agentKind); await settle();
  const reply = pending("plan_agent_lifecycle"); const preparing = controller.prepare(target, plan.action); reply.resolve(plan); await preparing;
}

test("lifecycle actions use backend availability and send the selected target evidence exactly", async (t) => {
  const context = fresh(t); const { value: controller } = lifecycleUi(context);
  const denied = lifecycleTarget({ id: "unverified-target", channel: "unverified", installation: assetInstallation("unverified-installation", { executablePath: "/fixture/native/codex" }),
    version: { state: "updateAvailable", source: "npmRegistry", latestVersion: "2.0.0", checkedAt: null, lastSuccessAt: null, nextCheckAt: null, stale: false, message: null },
    actions: [{ kind: "upgrade", available: false, reason: "provenanceUnverified", reasonMessage: "无法验证安装来源" }] });
  context.lifecycle.catalog = lifecycleCatalog([denied]); controller.open("codex"); await controller.prepare(denied, "upgrade");
  assert.equal(calls.filter((call) => call.command === "plan_agent_lifecycle").length, 0);
  const selected = lifecycleTarget({ id: "second-target", evidenceRevision: "second-target-evidence", channel: "npm",
    actions: [{ kind: "upgrade", available: true, reason: null, reasonMessage: null }] });
  context.lifecycle.catalog = lifecycleCatalog([denied, selected]);
  await prepareLifecycle(controller, selected, lifecyclePlan({ targetId: selected.id, action: "upgrade", channel: "npm" }));
  assert.deepEqual(calls.find((call) => call.command === "plan_agent_lifecycle")?.args, { request: {
    agentKind: "codex", targetId: "second-target", action: "upgrade", expectedEvidenceRevision: "second-target-evidence",
  } });
  assert.equal(controller.plan.value?.targetId, "second-target", controller.error.value);
});

test("lifecycle confirmation closes immediately and the backend operation survives page teardown", async (t) => {
  const context = fresh(t); const ui = lifecycleUi(context); const controller = ui.value;
  context.settings.settings.agentCliPaths.codex = "/fixture/previous/codex";
  await prepareLifecycle(controller);
  const apply = pending("apply_agent_lifecycle"); const poll = pending("get_agent_lifecycle_operation");
  controller.confirm();
  assert.equal(controller.visible.value, false); assert.equal(controller.plan.value, null);
  assert.equal(context.lifecycle.starting["npm-codex"], true);
  ui.stop(); context.navigation.openPage("mcp", "claudeCode");
  apply.resolve(lifecycleOperation());
  await until(() => calls.some((call) => call.command === "get_agent_lifecycle_operation"));
  poll.resolve(lifecycleOperation({ phase: "completed", revision: 2, canCancel: false, outcome: "appliedVerified",
    observedVersion: "1.1.0", verifiedExecutablePath: "/fixture/npm/bin/codex" }));
  await until(() => context.lifecycle.operations["lifecycle-operation"]?.phase === "completed");
  assert.equal(context.lifecycle.starting["npm-codex"], false); assert.equal(controller.visible.value, false);
  assert.equal(context.lifecycle.operations["lifecycle-operation"].observedVersion, "1.1.0");
  assert.equal(context.settings.settings.agentCliPaths.codex, "/fixture/previous/codex");
  assert.equal(calls.filter((call) => call.command === "save_settings").length, 0);
  assert.deepEqual(calls.find((call) => call.command === "apply_agent_lifecycle")?.args, { request: {
    planToken: "lifecycle-plan", agentKind: "codex", targetId: "npm-codex", action: "upgrade",
  } });
});

test("lifecycle preparation and submission timeouts release local state without retrying writes", async (t) => {
  const context = fresh(t); const { value: controller } = lifecycleUi(context);
  await fastTimeout(65_000, async () => {
    controller.open("codex"); const plan = pending("plan_agent_lifecycle"); await controller.prepare(lifecycleTarget(), "upgrade");
    assert.equal(controller.preparingTargetId.value, null); assert.match(controller.error.value, /超时/);
    controller.close(); plan.resolve(lifecyclePlan()); await settle();
    assert.equal(controller.plan.value, null); assert.equal(controller.visible.value, false);
  });
  for (const fails of [true, false]) {
    await prepareLifecycle(controller);
    await fastTimeout(15_000, async () => {
      const apply = pending("apply_agent_lifecycle"); controller.confirm();
      if (fails) apply.reject(new Error("fixture installer unavailable"));
      await until(() => !context.lifecycle.starting["npm-codex"]);
      assert.equal(controller.visible.value, false); assert.ok(context.lifecycle.startErrors["npm-codex"]);
      if (!fails) { apply.resolve(lifecycleOperation()); await settle(); }
      assert.equal(Object.keys(context.lifecycle.operations).length, 0);
    });
  }
  assert.equal(calls.filter((call) => call.command === "apply_agent_lifecycle").length, 2);
});

test("navigation and changed installation evidence invalidate lifecycle plans and ignore stale failures", async (t) => {
  const context = fresh(t); const { value: controller } = lifecycleUi(context);
  controller.open("codex");
  const first = pending("plan_agent_lifecycle"); const oldPlan = controller.prepare(lifecycleTarget(), "upgrade");
  context.navigation.openPage("skill"); first.reject(new Error("obsolete plan failure")); await oldPlan;
  assert.equal(controller.visible.value, false); assert.equal(controller.error.value, "");
  await prepareLifecycle(controller);
  const refreshReply = pending("get_agent_lifecycle_catalog"); const refreshing = context.lifecycle.refresh("force");
  assert.ok(controller.plan.value);
  context.lifecycle.invalidate();
  assert.equal(controller.plan.value, null);
  const newerReply = pending("get_agent_lifecycle_catalog"); const newerRefresh = context.lifecycle.refresh("force");
  refreshReply.reject(new Error("obsolete version failure")); await refreshing;
  const pendingPlan = pending("plan_agent_lifecycle"); const preparing = controller.prepare(lifecycleTarget(), "upgrade");
  pendingPlan.resolve(lifecyclePlan()); await preparing;
  backendLifecycle = lifecycleCatalog([lifecycleTarget({ evidenceRevision: "new-evidence" })]);
  newerReply.resolve(backendLifecycle); await newerRefresh;
  assert.equal(controller.plan.value, null); assert.equal(controller.preparingTargetId.value, null);
  assert.equal(context.lifecycle.catalog?.targets[0].evidenceRevision, "new-evidence");
  assert.equal(context.lifecycle.error, ""); assert.equal(context.lifecycle.loading, false);
});

test("normal lifecycle refreshes do not check remote versions or supersede an explicit check", async (t) => {
  const context = fresh(t);
  await context.lifecycle.refresh();
  assert.deepEqual(calls.find((call) => call.command === "get_agent_lifecycle_catalog")?.args, { request: { versionRefresh: "cached" } });
  const checkReply = pending("get_agent_lifecycle_catalog"); const checking = context.lifecycle.refresh("force");
  const revision = context.lifecycle.requestRevision;
  const passive = context.lifecycle.refresh(); assert.equal(context.lifecycle.requestRevision, revision);
  checkReply.resolve(lifecycleCatalog()); await Promise.all([checking, passive]);
  assert.equal(context.lifecycle.checkingVersions, false);
  assert.equal(calls.filter((call) => call.command === "get_agent_lifecycle_catalog").length, 2);
});

test("operation polling timeout releases tracking and late status cannot overwrite recovered results", async (t) => {
  const context = fresh(t);
  await fastTimeout(15_000, async () => {
    const status = pending("get_agent_lifecycle_operation"); context.lifecycle.track(lifecycleOperation());
    await until(() => Boolean(context.lifecycle.errors["lifecycle-operation"]));
    assert.equal(context.lifecycle.polling["lifecycle-operation"], false);
    assert.equal(context.lifecycle.operations["lifecycle-operation"].outcome, null);
    const recovery = pending("list_agent_lifecycle_operations"); const recovering = context.lifecycle.recover();
    recovery.resolve([lifecycleOperation({ phase: "completed", revision: 3, canCancel: false, outcome: "appliedUnverified" })]); await recovering;
    status.resolve(lifecycleOperation({ revision: 2 })); await settle();
    assert.equal(context.lifecycle.operations["lifecycle-operation"].revision, 3);
    assert.equal(context.lifecycle.operations["lifecycle-operation"].outcome, "appliedUnverified");
    assert.equal(context.lifecycle.errors["lifecycle-operation"], undefined);
    assert.equal(calls.filter((call) => call.command === "apply_agent_lifecycle").length, 0);
  });
});

test("operation identities and monotonic revisions reject stale or cross-target updates", async (t) => {
  const context = fresh(t);
  context.lifecycle.track(lifecycleOperation({ phase: "completed", revision: 4, canCancel: false, outcome: "appliedVerified" }));
  context.lifecycle.track(lifecycleOperation({ revision: 3 }));
  assert.equal(context.lifecycle.operations["lifecycle-operation"].revision, 4);
  assert.throws(() => context.lifecycle.track(lifecycleOperation({ revision: 5, targetId: "different-target" })), /目标不一致/);
  const reply = pending("get_agent_catalog_operation"); context.catalog.track(catalogOperation());
  await until(() => calls.some((call) => call.command === "get_agent_catalog_operation"));
  reply.resolve(catalogOperation({ id: "wrong-operation", revision: 2 }));
  await until(() => !context.catalog.polling["catalog-operation"]);
  assert.match(context.catalog.errors["catalog-operation"], /不同的目标/);
  assert.equal(context.catalog.operations["wrong-operation"], undefined);
});

test("cancel availability is backend-owned and timeout releases only the cancellation request", async (t) => {
  const context = fresh(t);
  context.lifecycle.operations = { "lifecycle-operation": lifecycleOperation({ canCancel: false }) };
  await context.lifecycle.cancel("lifecycle-operation"); assert.equal(calls.length, 0);
  context.lifecycle.operations = { "lifecycle-operation": lifecycleOperation() };
  await fastTimeout(15_000, async () => {
    const reply = pending("cancel_agent_lifecycle_operation"); const canceling = context.lifecycle.cancel("lifecycle-operation");
    assert.equal(context.lifecycle.canceling["lifecycle-operation"], true); await canceling;
    assert.equal(context.lifecycle.canceling["lifecycle-operation"], false); assert.match(context.lifecycle.errors["lifecycle-operation"], /超时/);
    reply.resolve(lifecycleOperation({ phase: "completed", revision: 2, outcome: "canceledBeforeCommit", canCancel: false })); await settle();
    assert.equal(context.lifecycle.operations["lifecycle-operation"].phase, "applying");
  });
});

test("Agent task disappearance is not inferred as success by the shared background center", async (t) => {
  const context = fresh(t);
  const module = await server.ssrLoadModule("/src/composables/useBackgroundTaskCenter.ts") as typeof import("../src/composables/useBackgroundTaskCenter.ts");
  const tasks = context.ui(() => backgrounds.useAgentBackgroundTasks()).value;
  const { value: center } = context.mount(() => module.useBackgroundTaskCenter({
    providers: ref([]), batchOperation: ref(null), batchOperationRunning: ref(false), batchOperationItems: ref([]), batchOperationError: ref(""), batchOperationCompleted: ref(false),
    openLoginAccount: () => {}, openProviderCredentials: () => {},
    refreshInProgress: ref(false), refreshingProviderIds: ref(new Set<string>()), checkInTasks: ref([]), checkInPending: ref([]),
    resumeCheckInTask: async () => {}, cancelCheckInTask: async () => {}, browserRuntime: ref(null), cancelBrowserRuntime: async () => {},
    checkingForUpdate: ref(false), updateCheckError: ref(""), installingUpdate: ref(false), updateDownloadProgress: ref(null), updateInstallStatus: ref(""), updateInstallError: ref(""),
    announcementsLoading: ref(false), announcementFatalError: ref(""), announcementErrors: ref([]), cliRuntimeLoading: ref(false), temporaryCliLaunchTasks: ref([]),
    probingCapabilitiesProviderId: ref(null), agentTasks: tasks,
  }));
  context.catalog.startTimes["global-asset"] = Date.now(); context.catalog.starting["global-asset"] = true; await nextTick();
  assert.equal(center.activeTaskCount.value, 1);
  context.catalog.starting["global-asset"] = false; await nextTick();
  assert.equal(center.activeTaskCount.value, 0); assert.equal(center.recentTasks.value.length, 0);
  context.catalog.startErrors["global-asset"] = "提交结果未知，请刷新"; await nextTick();
  assert.equal(center.recentTasks.value[0].status, "failed");
});

function dashboardUi(context: ReturnType<typeof fresh>) {
  return context.mount(() => dashboards.useAgentDashboard({
    active: computed(() => context.navigation.view === "agents"),
    cliRuntime: ref<CliRuntimeSnapshot>({ agents: [workspaceAgent("codex"), workspaceAgent("claudeCode")], configs: [] }),
    runtime: ref<AgentRuntimeSnapshot>({ schemaVersion: 1, revision: 1, updatedAt: Date.now(), sessions: [] }),
  }));
}



test("path save results belong to their Agent and cannot overwrite another drawer draft", async (t) => {
  const context = fresh(t); const { value: dashboard } = dashboardUi(context); await settle();
  dashboard.installation.open("codex"); dashboard.installation.pathDraft = "/fixture/new/codex";
  const saveReply = pending("save_settings"); const saving = dashboard.installation.savePath("codex", dashboard.installation.pathDraft);
  dashboard.installation.open("claudeCode"); dashboard.installation.pathDraft = "/fixture/claude-draft";
  saveReply.reject(new Error("fixture save failure")); await saving;
  assert.equal(dashboard.installation.savingPath, null); assert.equal(dashboard.installation.pathError, "");
  assert.match(dashboard.installation.pathErrors.codex!, /fixture save failure/);
  assert.equal(dashboard.installation.lifecycle.agentKind, "claudeCode"); assert.equal(dashboard.installation.pathDraft, "/fixture/claude-draft");
  const nextSave = pending("save_settings"); const probe = pending("probe_cli_tools");
  dashboard.installation.open("codex"); const nextSaving = dashboard.installation.savePath("codex", "/fixture/saved/codex");
  dashboard.installation.open("claudeCode"); dashboard.installation.pathDraft = "/fixture/new-claude-draft";
  nextSave.resolve({ ...context.settings.settings, agentCliPaths: { codex: "/fixture/saved/codex" } });
  probe.resolve({ tools: [{ ...workspaceAgent(), available: true, path: "/fixture/saved/codex", version: "1.1.0", message: "fixture" }] });
  await nextSaving;
  assert.equal(dashboard.installation.lifecycle.agentKind, "claudeCode"); assert.equal(dashboard.installation.pathDraft, "/fixture/new-claude-draft");
  dashboard.launchKind.value = "codex"; context.navigation.setView("agents"); context.navigation.setView("providers");
  assert.equal(dashboard.installation.lifecycle.agentKind, null); assert.equal(dashboard.launchKind.value, null);
});

test("path re-probe timeout releases busy state and rejects a late probe snapshot", async (t) => {
  const context = fresh(t); const { value: dashboard } = dashboardUi(context); await settle();
  await fastTimeout(20_000, async () => {
    const saveReply = pending("save_settings"); const probe = pending("probe_cli_tools");
    const saving = dashboard.installation.savePath("codex", "/fixture/new/codex");
    saveReply.resolve({ ...context.settings.settings, agentCliPaths: { codex: "/fixture/new/codex" } }); await saving;
    assert.equal(dashboard.installation.savingPath, null); assert.equal(context.cli.cliEnvironmentLoading, false);
    assert.match(dashboard.installation.pathErrors.codex!, /路径已保存.*超时/);
    probe.resolve({ tools: [{ ...workspaceAgent(), available: true, path: "/fixture/obsolete/codex", version: "0.1.0", message: "late" }] }); await settle();
    assert.equal(context.cli.cliEnvironmentProbe, null); assert.equal(context.settings.settings.agentCliPaths.codex, "/fixture/new/codex");
  });
});

test("late settings saves cannot overwrite a newer explicit path selection", async (t) => {
  const context = fresh(t); const old = pending("save_settings"); const newer = pending("save_settings");
  const firstSettings = { ...context.settings.settings, agentCliPaths: { codex: "/fixture/old/codex" } };
  const newSettings = { ...context.settings.settings, agentCliPaths: { codex: "/fixture/current/codex", claudeCode: "/fixture/current/claude" } };
  const first = context.settings.save(firstSettings); const second = context.settings.save(newSettings);
  newer.resolve(newSettings); await second; old.resolve(firstSettings); await first;
  assert.deepEqual(context.settings.settings.agentCliPaths, newSettings.agentCliPaths);
});

test("overview cards keep the selected version visible and move paths and diagnostics out of main content", async (t) => {
  const context = fresh(t);
  const html = await renderSurface("/src/components/agent-workspace/AgentOverviewCard.vue", { agent: workspaceAgent(), installations: backendCatalog.inventory.installations,
    lifecycleTargets: [], counts: { skill: 1, mcp: 2, extension: 0 }, inventoryReady: true, hookCount: null, runningCount: 0, canLaunch: true, loading: false,
    selectedPath: "/fixture/private/long/path/codex", selectedVersion: "1.2.3" }, context.pinia);
  const visible = html.replace(/<[^>]*>/g, "");
  assert.match(visible, /1\.2\.3/); assert.doesNotMatch(visible, /当前启动版本|尚未检查|已安装/); assert.doesNotMatch(visible, /\/fixture\//);
});

test("dashboard counts distinguish an unread inventory, an observed zero, and each selected scope", async (t) => {
  const context = fresh(t);
  context.navigation.setView("agents");
  context.catalog.catalogs = {}; context.environment.inventories = {}; context.environment.inventoryState = {};
  const first = pending("get_agent_asset_catalog");
  const { value: dashboard } = dashboardUi(context);
  assert.deepEqual(dashboard.cards.value[0].counts, { skill: null, mcp: null, extension: null });
  assert.equal(dashboard.cards.value[0].hookCount, null); assert.equal(dashboard.cards.value[0].inventoryReady, false);
  const firstRefresh = context.catalog.refresh();
  first.reject(new Error("fixture first read failed")); await firstRefresh;
  assert.equal(dashboard.loading.value, false); assert.match(dashboard.error.value, /first read failed/);
  assert.equal(dashboard.cards.value[0].counts.skill, null);
  backendCatalog = catalogSnapshot({ assets: [], counts: { codex: { skill: 0, mcp: 0, extension: 0 } } });
  backendCatalog.inventory.hookRuleCounts = [{ agentKind: "codex", ruleCount: 0 }];
  await dashboard.refresh();
  assert.deepEqual(dashboard.cards.value[0].counts, { skill: 0, mcp: 0, extension: 0 });
  assert.equal(dashboard.cards.value[0].hookCount, 0); assert.equal(dashboard.cards.value[0].inventoryReady, true);
  const project = pending("get_agent_asset_catalog");
  context.navigation.selectWorkspace("/fixture/project"); await settle();
  assert.equal(dashboard.cards.value[0].counts.skill, null); assert.equal(dashboard.cards.value[0].hookCount, null);
  context.navigation.selectWorkspace(); await settle();
  const projectSnapshot = catalogSnapshot();
  projectSnapshot.inventory.workspace = "/fixture/project";
  projectSnapshot.inventory.hookRuleCounts = [{ agentKind: "codex", ruleCount: 19 }];
  project.resolve(projectSnapshot); await settle();
  assert.equal(dashboard.cards.value[0].hookCount, 0);
  assert.equal(dashboard.cards.value[0].counts.mcp, 0);
});

function hookSnapshot() {
  const hooks = [
    assetRecord("hook-codex", { category: "hook", nativeId: "codex-hook", label: "Codex native rules", details: { kind: "hook", managed: false, enabled: "enabled", ruleCount: 5 } }),
    assetRecord("hook-claude", { agentKind: "claudeCode", category: "hook", nativeId: "claude-hook", label: "Claude policy rules", scope: "managed", details: { kind: "hook", managed: true, enabled: "enabled", ruleCount: 3 } }),
    assetRecord("status-ui", { category: "statusUi", nativeId: "status-line", label: "Not a Hook", details: { kind: "statusUi", mode: "command", commandPresent: true } }),
  ];
  return catalogSnapshot({ assets: hooks.map((native) => catalogAsset(`global-${native.stableId}`, { name: native.label, category: native.category, ownership: "observed", version: null, application: { available: false, sourceBindingId: null, targets: [], observations: catalogObservations([native.agentKind]), reason: "尚未收录" }, bindings: [{ ...catalogBinding(native.agentKind, native.stableId), native, actions: native.actions }], variants: [] })), inventory: assetInventory({ assets: hooks, hookRuleCounts: [{ agentKind: "codex", ruleCount: 37 }, { agentKind: "claudeCode", ruleCount: null }] }) });
}

test("Hook cards consume the Rust rule summary and source navigation retains the native target", async (t) => {
  const context = fresh(t, hookSnapshot()); const { value: dashboard } = dashboardUi(context); await settle();
  assert.equal(dashboard.cards.value.find((card) => card.agent.kind === "codex")?.hookCount, 37);
  assert.equal(dashboard.cards.value.find((card) => card.agent.kind === "claudeCode")?.hookCount, null);
  dashboard.environment.assets.openDetail("hook-claude");
  const facts = dashboard.environment.assets.selectedDetail?.facts;
  assert.ok(facts); assert.doesNotMatch(JSON.stringify(facts), /BalanceHub 管理/);
  assert.ok(facts.some((fact) => fact.label === "已配置规则" && fact.value === "3 条"));
  dashboard.showHook("hook-claude");
  assert.equal(context.navigation.page, "hook"); assert.equal(context.navigation.agentFilter, "claudeCode");
  assert.equal(dashboard.focusedHookId.value, "hook-claude"); assert.equal(dashboard.environment.assets.drawerVisible, false);
  context.navigation.openPage("skill"); assert.equal(dashboard.focusedHookId.value, null);
});

test("Hook page filters native rows, separates Status UI and lazily loads optional integration details", async (t) => {
  const context = fresh(t, hookSnapshot()); const { value: dashboard } = dashboardUi(context); await settle();
  const props = { catalog: hookSnapshot(), loading: false, error: "", busy: () => false, agents: [workspaceAgent(), workspaceAgent("claudeCode")], agentFilter: "codex", query: "", focusedAssetId: "hook-codex", center: dashboard.environment, hooks: dashboard.hooks };
  const html = await renderSurface("/src/components/agent-workspace/AgentHookPanel.vue", props, context.pinia);
  assert.match(html, /data-global-asset-id="global-hook-codex"/); assert.match(html, /aria-current="true"/);
  assert.doesNotMatch(html, /data-global-asset-id="global-hook-claude"/);
  assert.match(html, /可选辅助功能/);
  assert.match(html, /<details class="agent-hook-integration"/); assert.doesNotMatch(html, /<details[^>]+open/);
  assert.doesNotMatch(html, /\/fixture\/codex\/hooks.json/, "collapsed integration does not load native details");
  const empty = catalogSnapshot({ assets: [] });
  empty.inventory.diagnostics = [{ kind: "discoveryIncomplete", agentKind: "codex", category: "hook", reason: "unsupportedVersion" }];
  context.environment.inventories.__native__ = empty.inventory; props.catalog = empty;
  const incomplete = await renderSurface("/src/components/agent-workspace/AgentHookPanel.vue", props, context.pinia);
  assert.match(incomplete, /当前版本/); assert.match(incomplete, /没有匹配的 Hook/);
  assert.doesNotMatch(incomplete, /当前范围内未发现 Hook 配置/);
  const unreadable = assetInventory({ assets: [], sources: [assetSource("unreadable", { categories: ["hook"], diagnostics: [{ kind: "readFailed", sourceId: "unreadable", errorKind: "permissionDenied" }] })],
    hookRuleCounts: [{ agentKind: "codex", ruleCount: null }] });
  context.environment.inventories.__native__ = unreadable; props.catalog = catalogSnapshot({ assets: [], inventory: unreadable });
  const failed = await renderSurface("/src/components/agent-workspace/AgentHookPanel.vue", props, context.pinia);
  assert.match(failed, /Hook 规则数量尚未确认/);
  assert.doesNotMatch(failed, /当前范围内未发现 Hook 配置/);
});

test("managed Hook cleanup without an installation renders only the actions authorized by inspection", async (t) => {
  const context = fresh(t);
  const inspection = { ...workspaceHook("codex"), installed: true, enabled: true, state: "installed_unverified",
    actions: [{ action: "install", available: false, reason: "没有可用 CLI" }, { action: "remove", available: true, reason: null }, { action: "disable", available: true, reason: null }] };
  const html = await renderSurface("/src/components/settings/agent-environment/AgentEnvironmentRow.vue", {
    agent: workspaceAgent(), installations: [], inspection, busy: false, error: null,
  }, context.pinia);
  assert.match(html, /Codex CLI/); assert.match(html, /删除 Hook/); assert.match(html, /启用或停用 BalanceHub Hook/);
  assert.doesNotMatch(html, /安装 Hook|查看 Agent 详情|修复 Hook/);
});

function hookPlan(kind: AgentCliKind = "codex"): AgentHookPlan {
  return { agentKind: kind, mutation: "install", runtimeScope: { kind: "native" }, configPath: `/fixture/${kind}/hooks.json`,
    expectedRevision: "hook-fixture", supported: true, conflict: false,
    changes: [{ eventName: "SessionStart", structuralIdentity: "fixture-handler", fingerprint: "fixture-fingerprint", kind: "add" }], contentChanges: [], summary: "安装测试集成" };
}

test("pending Hook apply closes its modal, preserves navigation, ignores pre-commit refresh and rechecks after completion", async (t) => {
  const context = fresh(t, hookSnapshot()); const { value: dashboard } = dashboardUi(context); await settle();
  const plan = pending("plan_agent_hook"); const planning = dashboard.hooks.requestPlan("codex", "install");
  plan.resolve(hookPlan()); await planning;
  const apply = pending("apply_agent_hook"); const applying = dashboard.confirmHookPlan();
  assert.equal(dashboard.hooks.planVisible, false); assert.equal(dashboard.hooks.pendingPlan, null);
  assert.equal(dashboard.hooks.isRowBusy("codex:native"), true); assert.equal(dashboard.hooks.isRowBusy("claudeCode:native"), false);
  const previousReads = calls.filter((call) => call.command === "inspect_agent_hook" && call.args.agentKind === "codex").length;
  await dashboard.refresh();
  assert.equal(calls.filter((call) => call.command === "inspect_agent_hook" && call.args.agentKind === "codex").length, previousReads);
  assert.equal(dashboard.hooks.isRowBusy("codex:native"), true);
  context.navigation.openPage("hook", "claudeCode"); context.navigation.selectWorkspace("/fixture/project"); await settle();
  assert.equal(context.navigation.page, "hook"); assert.equal(context.navigation.agentFilter, "claudeCode");
  const health = pending("health_agent_hook");
  const installed = { ...workspaceHook("codex"), revision: "verified-after-apply", installed: true, enabled: true };
  const preCommitReply = pending("get_agent_asset_catalog");
  const preCommitSnapshot = structuredClone(backendCatalog);
  const preCommitRefresh = context.catalog.refresh("/fixture/project");
  backendCatalog.inventory.hookRuleCounts = [{ agentKind: "codex", ruleCount: 41 }];
  apply.resolve(installed); await settle();
  assert.ok(calls.some((call) => call.command === "health_agent_hook"));
  assert.equal(dashboard.hooks.isRowBusy("codex:native"), true);
  health.resolve(installed); await settle();
  preCommitReply.resolve(preCommitSnapshot);
  await preCommitRefresh; await applying;
  assert.equal(dashboard.hooks.isRowBusy("codex:native"), false);
  assert.equal(dashboard.hooks.inspectionFor("codex")?.revision, "verified-after-apply");
  assert.equal(context.catalog.catalogs["/fixture/project"].inventory.hookRuleCounts.find(entry => entry.agentKind === "codex")?.ruleCount, 41);
  assert.equal(context.navigation.agentFilter, "claudeCode");
  assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").at(-1)?.args.workspace, "/fixture/project");
});

test("Hook apply failures and timeouts release state, retain their error and reject late snapshots", async (t) => {
  const context = fresh(t); const { value: dashboard } = dashboardUi(context); await settle();
  for (const timeout of [false, true]) {
    const plan = pending("plan_agent_hook"); const planning = dashboard.hooks.requestPlan("codex", "install");
    plan.resolve(hookPlan()); await planning;
    const apply = pending("apply_agent_hook");
    await fastTimeout(15_000, async () => {
      const applying = dashboard.confirmHookPlan();
      if (!timeout) apply.reject(new Error("fixture apply failure"));
      await applying;
    });
    assert.equal(dashboard.hooks.isRowBusy("codex:native"), false); assert.equal(dashboard.hooks.planVisible, false);
    assert.match(dashboard.hooks.rowError("codex:native") || "", timeout ? /超时/ : /fixture apply failure/);
    if (timeout) {
      apply.resolve({ ...workspaceHook("codex"), revision: "late-obsolete-result", installed: true }); await settle();
      assert.notEqual(dashboard.hooks.inspectionFor("codex")?.revision, "late-obsolete-result");
    }
  }
});
