import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test, { after, before, type TestContext } from "node:test";
import { compileScript, parse } from "@vue/compiler-sfc";
import { createPinia, disposePinia, setActivePinia } from "pinia";
import { createRenderer, defineComponent, h, nextTick, reactive, ref, type Component } from "vue";
import { createServer, transformWithEsbuild, type ViteDevServer } from "vite";
import type { AgentSessionCancelRequest, AgentSessionCount, AgentSessionCountRequest, AgentSessionCounts, AgentSessionQuery, AgentSessionScope } from "../src/stores/agent-session-types.ts";
import type { AgentCliKind, AgentRuntimeSnapshot, CliRuntimeSnapshot } from "../src/stores/provider-types.ts";
import { runtimeSession, sessionPage, sessionRow, sessionScope } from "./agent-session-fixtures.ts";
import { catalogSnapshot, lifecycleCatalog, workspaceAgent, workspaceHook } from "./agent-workspace-fixtures.ts";

type NavigationModule = typeof import("../src/stores/agent-workspace.ts");
type CatalogModule = typeof import("../src/stores/agent-catalog.ts");
type EnvironmentModule = typeof import("../src/stores/agent-environment.ts");
type LifecycleModule = typeof import("../src/stores/agent-lifecycle.ts");
type WorkspaceModule = typeof import("../src/stores/workspaces.ts");
type CliModule = typeof import("../src/stores/cli-runtime.ts");
type Deferred<T> = { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void };
type HostNode = { type: string; text: string; props: Record<string, unknown>; children: HostNode[]; parent: HostNode | null };

const dashboardFile = fileURLToPath(new URL("../src/components/agent-workspace/AgentDashboard.vue", import.meta.url));
const componentFiles = new Set([
  dashboardFile,
  fileURLToPath(new URL("../src/components/agent-workspace/AgentOverviewCard.vue", import.meta.url)),
  fileURLToPath(new URL("../src/components/agent-workspace/AgentSessionPanel.vue", import.meta.url)),
  fileURLToPath(new URL("../src/components/agent-workspace/AgentWorkspaceIcon.vue", import.meta.url)),
  fileURLToPath(new URL("../src/components/AgentCliIcon.vue", import.meta.url)),
]);
const kinds: AgentCliKind[] = ["codex", "claudeCode", "gemini", "grok"];
const counts: AgentSessionCount[] = [
  { agentKind: "codex", loadedCount: 155, total: 155 },
  { agentKind: "claudeCode", loadedCount: 7, total: null },
  { agentKind: "gemini", loadedCount: 0, total: 0 },
  { agentKind: "grok", loadedCount: 0, total: null },
];
const queries: AgentSessionQuery[] = [];
const scopeRequests: (string | null)[] = [];
const countRequests: AgentSessionCountRequest[] = [];
const cancellations: AgentSessionCancelRequest[] = [];
const replies: Promise<AgentSessionCounts>[] = [];
const unexpected: string[] = [];
const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
let backendScope: AgentSessionScope;
let server: ViteDevServer;
let Dashboard: Component;
let navigationModule: NavigationModule;
let catalogModule: CatalogModule;
let environmentModule: EnvironmentModule;
let lifecycleModule: LifecycleModule;
let workspaceModule: WorkspaceModule;
let cliModule: CliModule;
const previousGlobals = new Map<string, PropertyDescriptor | undefined>(["window", "__TAURI_INTERNALS__", "__BALANCEHUB_HISTORY_LISTENERS__"].map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));

before(async () => {
  Object.assign(globalThis, { Element: class {}, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } });
  const windowEvents = new EventTarget();
  const documentEvents = new EventTarget();
  Object.assign(globalThis, { addEventListener: windowEvents.addEventListener.bind(windowEvents), removeEventListener: windowEvents.removeEventListener.bind(windowEvents), document: { body: { classList: { add() {}, remove() {} } }, documentElement: { style: {} }, visibilityState: "visible", addEventListener: documentEvents.addEventListener.bind(documentEvents), removeEventListener: documentEvents.removeEventListener.bind(documentEvents) } });
  Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
  Object.defineProperty(globalThis, "__BALANCEHUB_HISTORY_LISTENERS__", { value: listeners, configurable: true });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", { configurable: true, value: {
    transformCallback() { return 1; }, unregisterCallback() {},
    invoke(command: string, args: Record<string, unknown> = {}) {
      if (command === "get_agent_session_scope") { scopeRequests.push(args.explicitWorkdir as string | null); return Promise.resolve(structuredClone(backendScope)); }
      if (command === "query_agent_sessions") {
        const request = args.request as AgentSessionQuery;
        queries.push(structuredClone(request));
        return Promise.resolve(historyPage(request.scopeRevision));
      }
      if (command === "count_agent_sessions") {
        countRequests.push(structuredClone(args.request as AgentSessionCountRequest));
        return replies.shift() ?? Promise.resolve(countResult());
      }
      if (command === "cancel_agent_session_query") { cancellations.push(args.request as AgentSessionCancelRequest); return Promise.resolve(); }
      if (command === "get_agent_asset_catalog") {
        const catalog = catalogSnapshot(); catalog.inventory.workspace = args.workspace as string | null;
        return Promise.resolve(catalog);
      }
      if (command === "get_cached_agent_overview") return Promise.resolve([]);
      if (command === "has_agent_catalog_changes") return Promise.resolve(false);
      if (command === "refresh_agent_overview") return Promise.resolve({ agentKind: args.agentKind, probe: { ...workspaceAgent(args.agentKind as AgentCliKind), available: true, path: "/fixture/bin/agent", version: "1.0.0", message: "fixture" }, configuration: null, configurationError: "", updatedAt: new Date().toISOString() });
      if (command === "get_agent_lifecycle_catalog") return Promise.resolve(lifecycleCatalog());
      if (command === "list_agent_configuration_sources") return Promise.resolve(null);
      if (["list_agent_catalog_operations", "list_agent_lifecycle_operations", "list_agent_asset_operations", "list_agent_session_resume_operations"].includes(command)) return Promise.resolve([]);
      if (command === "inspect_agent_hook" || command === "health_agent_hook") return Promise.resolve(workspaceHook(args.agentKind as AgentCliKind));
      unexpected.push(command);
      return Promise.reject(new Error(`Unexpected Tauri command: ${command}`));
    },
  } });
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    configFile: false, server: { middlewareMode: true, hmr: false }, appType: "custom", logLevel: "silent",
    resolve: { alias: [
      { find: /^@arco-design\/web-vue$/, replacement: "virtual:history-messages" },
      { find: /^@arco-design\/web-vue\/es\/icon$/, replacement: "virtual:history-icons" },
      { find: /^@tauri-apps\/api\/event$/, replacement: "virtual:history-events" },
    ] },
    plugins: [{
      name: "agent-history-component-tests",
      resolveId(id) { if (id.startsWith("virtual:history-")) return `\0${id.slice("virtual:".length)}`; },
      async load(id) {
        if (id === "\0history-messages") return "export const Message = { success() {}, error() {}, info() {}, warning() {} };";
        if (id === "\0history-icons") return "export const IconCommand = { render() { return null; } };";
        if (id === "\0history-events") return `export async function listen(event, handler) {
          const registry = globalThis.__BALANCEHUB_HISTORY_LISTENERS__;
          const handlers = registry.get(event) ?? new Set(); handlers.add(handler); registry.set(event, handlers);
          return () => handlers.delete(handler);
        }`;
        if (!id.endsWith(".vue")) return;
        // Mount the actual count, navigation and panel templates. Unrelated
        // drawers stay inert so these tests need neither DOM nor a Tauri App.
        if (!componentFiles.has(id) && !id.includes("/workspace-card/") && !/\/(AgentCardActions|AgentCardConfigurationSummary|AgentVersionStatus)\.vue$/.test(id)) return "export default { render() { return null; } };";
        const { descriptor, errors } = parse(readFileSync(id, "utf8").replace(/<(\/?)TransitionGroup\b/g, "<$1div"), { filename: id });
        assert.deepEqual(errors, []);
        const compiled = compileScript(descriptor, { fs: { fileExists: existsSync, readFile: (path) => readFileSync(path, "utf8") }, id, inlineTemplate: true, templateOptions: { compilerOptions: { hoistStatic: false } } });
        return (await transformWithEsbuild(compiled.content, `${id}.ts`, { loader: "ts", target: "esnext" })).code;
      },
    }],
  });
  [navigationModule, catalogModule, environmentModule, lifecycleModule, workspaceModule, cliModule] = await Promise.all([
    server.ssrLoadModule("/src/stores/agent-workspace.ts"), server.ssrLoadModule("/src/stores/agent-catalog.ts"),
    server.ssrLoadModule("/src/stores/agent-environment.ts"), server.ssrLoadModule("/src/stores/agent-lifecycle.ts"),
    server.ssrLoadModule("/src/stores/workspaces.ts"), server.ssrLoadModule("/src/stores/cli-runtime.ts"),
  ]) as [NavigationModule, CatalogModule, EnvironmentModule, LifecycleModule, WorkspaceModule, CliModule];
  Dashboard = (await server.ssrLoadModule(dashboardFile)).default as Component;
});

after(async () => {
  await server?.close();
  for (const [key, descriptor] of previousGlobals) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor);
    else Reflect.deleteProperty(globalThis, key);
  }
});

function countResult(): AgentSessionCounts {
  return { counts: structuredClone(counts), errors: { claudeCode: "原生会话身份不可读", grok: "原生目录不可读" } };
}
function historyPage(revision = backendScope.revision) {
  return sessionPage([sessionRow()], {
    scopeRevision: revision, agentCounts: structuredClone(counts), loadedCount: 162, total: null, nextCursor: "snapshot:1:1",
    sourceStates: [{ sourceId: "source:codex", workspaceId: "home", state: "complete", loadedCount: 999, indexState: "ready", message: null }],
  });
}
function node(type: string, text = ""): HostNode { return { type, text, props: {}, children: [], parent: null, style: {}, getBoundingClientRect: () => ({ top: 0, left: 0, width: 0, height: 0 }) } as HostNode; }
function remove(child: HostNode) {
  if (child.parent) child.parent.children = child.parent.children.filter((item) => item !== child);
  child.parent = null;
}
function insert(child: HostNode, parent: HostNode, anchor: HostNode | null = null) {
  remove(child);
  const position = anchor ? parent.children.indexOf(anchor) : -1;
  parent.children.splice(position < 0 ? parent.children.length : position, 0, child);
  child.parent = parent;
}
const teleportHost = node("body");
const renderer = createRenderer<HostNode, HostNode>({
  querySelector: (selector) => selector === "body" ? teleportHost : null,
  patchProp(element, key, _previous, value) { element.props[key] = value; }, insert, remove,
  createElement: (type) => node(type), createText: (text) => node("#text", text), createComment: (text) => node("#comment", text),
  setText(element, text) { element.text = text; },
  setElementText(element, text) { element.children.forEach((child) => { child.parent = null; }); element.children = []; element.text = text; },
  parentNode: (element) => element.parent,
  nextSibling(element) { if (!element.parent) return null; return element.parent.children[element.parent.children.indexOf(element) + 1] ?? null; },
  insertStaticContent(content, parent, anchor) { const child = node("#static", content.replace(/<[^>]*>/g, "")); insert(child, parent, anchor); return [child, child]; },
});
function descendants(root: HostNode): HostNode[] { return [root, ...root.children.flatMap(descendants)]; }
function text(root: HostNode): string { return root.type === "#comment" ? "" : root.text + root.children.map(text).join(""); }
function click(button: HostNode) {
  assert.ok(button.props.disabled !== true && button.props.disabled !== "", "navigation stays usable while counting");
  assert.equal(typeof button.props.onClick, "function");
  (button.props.onClick as (event: { type: string }) => void)({ type: "click" });
}
function deferred<T>(): Deferred<T> {
  let resolve!: Deferred<T>["resolve"]; let reject!: Deferred<T>["reject"];
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
async function settle() { for (let index = 0; index < 24; index += 1) { await Promise.resolve(); await nextTick(); } }

function mountDashboard(t: TestContext, options: { visible?: boolean; firstReply?: Deferred<AgentSessionCounts> } = {}) {
  queries.length = 0; scopeRequests.length = 0; countRequests.length = 0; cancellations.length = 0; replies.length = 0; unexpected.length = 0; listeners.clear();
  backendScope = sessionScope();
  backendScope.sources.push(...(["gemini", "grok"] as const).map((kind) => ({ id: `source:${kind}`, agentKind: kind, configRoot: `/fixture/${kind}`, available: true })));
  if (options.firstReply) replies.push(options.firstReply.promise);
  const pinia = createPinia(); setActivePinia(pinia);
  const navigation = navigationModule.useAgentWorkspaceStore();
  navigation.setView(options.visible === false ? "providers" : "agents");
  const catalog = catalogModule.useAgentCatalogStore(); const environment = environmentModule.useAgentEnvironmentStore();
  const snapshot = catalogSnapshot(); catalog.catalogs.__native__ = snapshot;
  environment.inventories.__native__ = snapshot.inventory; environment.inventoryState.__native__ = "ready";
  const lifecycle = lifecycleModule.useAgentLifecycleStore(); lifecycle.catalog = lifecycleCatalog();
  const workspaces = workspaceModule.useWorkspaceStore();
  workspaces.workspaces = [{ path: "/fixture/home/project", useCount: 1 }, { path: "/fixture/unmounted/project", useCount: 2 }];
  const cli = cliModule.useCliRuntimeStore();
  const props = reactive({
    active: true,
    cliRuntime: { agents: kinds.map(workspaceAgent), configs: [] } as CliRuntimeSnapshot,
    runtimeSnapshot: { schemaVersion: 1, revision: 1, updatedAt: 1, sessions: [] } as AgentRuntimeSnapshot,
  });
  const exposed = ref<{ refresh: () => Promise<void> | undefined } | null>(null);
  const app = renderer.createApp(defineComponent({ setup() { return () => h(Dashboard, {
    ...props, active: props.active && navigation.view === "agents", providers: [], runtimeLoading: false, activatingId: null, ref: exposed,
  }); } }));
  app.use(pinia);
  for (const [name, tag] of [["a-button", "button"], ["a-select", "select"], ["a-option", "option"], ["a-input", "input"], ["a-tooltip", "span"]]) {
    app.component(name, defineComponent({ inheritAttrs: false, setup(_, { attrs, slots }) { return () => h(tag, attrs, [slots.icon?.(), slots.default?.()]); } }));
  }
  const root = node("root"); app.mount(root);
  t.after(async () => {
    options.firstReply?.resolve(countResult()); app.unmount(); disposePinia(pinia); await settle();
    assert.deepEqual(unexpected, []);
  });
  function historyButton(kind: AgentCliKind) {
    const label = `查看 ${workspaceAgent(kind).label} 的 历史会话，`;
    const found = descendants(root).filter((item) => item.type === "button" && String(item.props["aria-label"] ?? "").startsWith(label));
    assert.equal(found.length, 1); return found[0];
  }
  function value(kind: AgentCliKind) {
    const count = descendants(historyButton(kind)).find((item) => item.type === "strong");
    assert.ok(count); return text(count);
  }
  function navButton(label: string) {
    const nav = descendants(root).find((item) => item.props["aria-label"] === "Agent 管理分类");
    assert.ok(nav);
    const button = descendants(nav).find((item) => item.type === "button" && text(item) === label);
    assert.ok(button); return button;
  }
  function sessionTab(label: string) {
    const tabs = descendants(root).find((item) => item.props["aria-label"] === "会话视图");
    assert.ok(tabs);
    const button = descendants(tabs).find((item) => item.type === "button" && text(item) === label);
    assert.ok(button); return button;
  }
  function historyPanel() {
    const panel = descendants(root).find((item) => item.props["aria-label"] === "Agent 会话");
    assert.ok(panel); return panel;
  }
  return { root, navigation, props, workspaces, cli, lifecycle, exposed, historyButton, value, navButton, sessionTab, historyPanel };
}

test("history survives active, asset and provider navigation without rereading or using their search text", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const surface = mountDashboard(t); await settle();
  assert.equal(queries.length, 0, "history stays unmounted until its first visit");
  click(surface.navButton("会话")); await settle();
  assert.equal(queries.length, 1);
  const panel = surface.historyPanel();
  surface.navigation.query = "history keyword";
  t.mock.timers.tick(280); await settle();
  assert.equal(queries.length, 2);
  assert.equal(queries[1].query, "history keyword");

  click(surface.sessionTab("活动会话")); await settle();
  surface.navigation.query = "active keyword";
  t.mock.timers.tick(1000); await settle();
  click(surface.sessionTab("历史会话")); await settle();
  assert.equal(surface.navigation.query, "history keyword");
  assert.equal(queries.length, 2);

  click(surface.navButton("Skill")); await settle();
  surface.navigation.query = "skill keyword";
  t.mock.timers.tick(1000); await settle();
  click(surface.navButton("会话")); await settle();
  assert.equal(surface.navigation.query, "history keyword");
  assert.equal(surface.historyPanel(), panel, "the previously opened session panel is retained");
  assert.match(text(panel), /已显示 1 条/);
  assert.equal(queries.length, 2);

  surface.navigation.setView("providers"); await settle();
  surface.navigation.setView("agents"); await settle();
  surface.props.active = false; await settle();
  surface.props.active = true; await settle();
  click(surface.navButton("会话")); await settle();
  assert.equal(queries.length, 2);
  assert.equal(scopeRequests.length, 1);
  assert.equal(surface.historyPanel(), panel);
});

test("cached history preserves its selected directory and card navigation can explicitly restore all directories", async (t) => {
  const surface = mountDashboard(t); await settle();
  click(surface.navButton("会话")); await settle();
  const directory = descendants(surface.root).find((item) => item.props["aria-label"] === "会话工作目录");
  assert.ok(directory);
  (directory.props.onChange as (id: string) => void)("project"); await settle();
  assert.equal(queries.length, 2);
  assert.deepEqual(queries[1].workspaceIds, ["project"]);
  click(surface.navButton("总览")); await settle();
  click(surface.navButton("会话")); await settle();
  assert.equal(queries.length, 2);
  assert.equal(surface.navigation.sessionWorkspaceSelection, "project");
  assert.equal(surface.navigation.sessionWorkspaceMode, "home");
  click(surface.navButton("总览")); await settle();
  click(surface.historyButton("codex")); await settle();
  assert.equal(queries.length, 3, "only the final Agent and directory selection is queried");
  assert.deepEqual(queries[2].workspaceIds, ["home", "project", "unmounted"]);
  assert.deepEqual(queries[2].agentKinds, ["codex"]);
  assert.equal(surface.navigation.sessionWorkspaceMode, "all");
  click(surface.navButton("共享库")); await settle();
  surface.navigation.selectAgent("gemini"); await settle();
  click(surface.navButton("会话")); await settle();
  assert.equal(surface.navigation.agentFilter, "codex", "other pages have their own Agent selection");
  assert.equal(queries.length, 3);
});

test("real index and recorded-directory changes invalidate hidden history without scanning until it is visible", async (t) => {
  const surface = mountDashboard(t); await settle();
  click(surface.historyButton("codex")); await settle();
  assert.equal(queries.length, 1);
  click(surface.navButton("总览")); await settle();
  for (const handler of listeners.get("cli-session-index-updated") ?? []) handler({ payload: "gemini" });
  click(surface.navButton("会话")); await settle();
  assert.equal(queries.length, 1, "unrelated Agent index changes do not invalidate this history");
  click(surface.navButton("总览")); await settle();
  for (const handler of listeners.get("cli-session-index-updated") ?? []) handler({ payload: null });
  await settle();
  assert.equal(queries.length, 1, "global cache reconfiguration is deferred while hidden");
  click(surface.navButton("会话")); await settle();
  assert.equal(queries.length, 2);
  click(surface.navButton("总览")); await settle();
  backendScope = { ...backendScope, revision: "scope:changed", workspaces: [...backendScope.workspaces, { id: "new", path: "/fixture/new", isHome: false, exists: true }] };
  surface.workspaces.workspaces.push({ path: "/fixture/new", useCount: 1 }); await settle();
  assert.equal(queries.length, 2);
  click(surface.navButton("会话")); await settle();
  assert.equal(queries.length, 3);
  assert.equal(queries[2].scopeRevision, "scope:changed");
  assert.deepEqual(queries[2].workspaceIds, ["home", "project", "unmounted", "new"]);
});

test("one visible overview consumer serves four cards without rescanning for runtime, filters, versions, asset scope or index events", async (t) => {
  const surface = mountDashboard(t, { visible: false });
  await settle(); assert.equal(countRequests.length, 0, "a mounted overview hidden behind providers does not scan");
  surface.navigation.setView("agents"); await settle();
  assert.equal(countRequests.length, 1);
  assert.deepEqual(countRequests[0].agentKinds, kinds);
  assert.equal(queries.length, 0, "overview counts do not read a history page");
  assert.deepEqual(kinds.map(surface.value), ["155", "读取失败", "0", "读取失败"]);
  surface.props.runtimeSnapshot = { schemaVersion: 1, revision: 2, updatedAt: 2, sessions: [
    runtimeSession("confirmed", "codex", { state: "busy" }),
    runtimeSession("unknown", "codex", { state: "unknown" }),
    runtimeSession("ended", "codex", { state: "ended", endedAt: 2 }),
  ] };
  surface.navigation.query = "Codex"; surface.navigation.selectAgent("codex"); await settle();
  assert.equal(surface.value("codex"), "155");
  const active = descendants(surface.root).find((item) => item.type === "button" && String(item.props["aria-label"] ?? "").startsWith("查看 Codex CLI 的 活跃会话"));
  assert.ok(active); assert.match(text(active), /活跃1$/);
  surface.navigation.query = ""; surface.navigation.selectAgent(null);
  surface.navigation.selectWorkspace("/fixture/home/project");
  surface.cli.cliEnvironmentProbe = { tools: [{ ...workspaceAgent(), available: true, path: "/fixture/codex", version: "2.0.0", message: "fixture" }] };
  await surface.lifecycle.refresh(true); await settle();
  for (const handler of listeners.get("cli-session-index-updated") ?? []) handler({ payload: "codex" });
  await settle();
  assert.equal(countRequests.length, 1);
  assert.equal(queries.length, 0);
  assert.equal(listeners.get("cli-session-index-updated")?.size ?? 0, 0, "overview counts do not subscribe to index refreshes");
  assert.deepEqual(kinds.map(surface.value), ["155", "读取失败", "0", "读取失败"]);
});

test("overview refreshes for directory-set changes, explicit refresh and reactivation", async (t) => {
  const surface = mountDashboard(t); await settle(); assert.equal(countRequests.length, 1);
  surface.workspaces.workspaces = [...surface.workspaces.workspaces].reverse().map((workspace) => ({ ...workspace, useCount: workspace.useCount + 1 }));
  await settle(); assert.equal(countRequests.length, 1, "ordering and use counts do not change the directory set");
  backendScope = { ...backendScope, revision: "scope:added", workspaces: [...backendScope.workspaces, { id: "new", path: "/fixture/new", isHome: false, exists: true }] };
  surface.workspaces.workspaces.push({ path: "/fixture/new", useCount: 1 }); await settle();
  assert.equal(countRequests.length, 2);
  backendScope = { ...backendScope, revision: "scope:forgotten", workspaces: backendScope.workspaces.filter((workspace) => workspace.id !== "unmounted") };
  surface.workspaces.workspaces = surface.workspaces.workspaces.filter((workspace) => workspace.path !== "/fixture/unmounted/project");
  await settle(); assert.equal(countRequests.length, 3);
  await surface.exposed.value?.refresh(); await settle(); assert.equal(countRequests.length, 4);
  click(surface.navButton("Skill")); await settle(); assert.equal(countRequests.length, 4);
  click(surface.navButton("总览")); await settle(); assert.equal(countRequests.length, 5);
  surface.navigation.setView("providers"); await settle(); assert.equal(countRequests.length, 5);
  surface.navigation.setView("agents"); await settle(); assert.equal(countRequests.length, 6);
  assert.equal(new Set(countRequests.map((request) => request.consumerId)).size, 1);
  assert.ok(countRequests.every((request) => request.agentKinds.join("|") === kinds.join("|")));
  assert.equal(queries.length, 0);
});

test("pending card history clicks navigate immediately and the first panel request uses every workspace", async (t) => {
  const pending = deferred<AgentSessionCounts>();
  const surface = mountDashboard(t, { firstReply: pending }); await settle();
  assert.equal(countRequests.length, 1); const overview = countRequests[0];
  assert.equal(surface.historyButton("codex").props["aria-busy"], true);
  surface.navigation.query = "Codex"; await settle();
  click(surface.historyButton("codex"));
  assert.equal(surface.navigation.page, "sessions"); assert.equal(surface.navigation.sessionView, "history");
  assert.equal(surface.navigation.query, ""); assert.equal(surface.navigation.sessionWorkspaceMode, "all");
  await settle();
  assert.ok(cancellations.some((request) => request.consumerId === overview.consumerId && request.requestId === overview.requestId));
  assert.equal(queries.length, 1, "no intermediate HOME request is issued");
  assert.notEqual(queries[0].consumerId, overview.consumerId);
  assert.deepEqual(queries[0].workspaceIds, ["home", "project", "unmounted"]);
  assert.deepEqual(queries[0].agentKinds, ["codex"]);
  assert.equal(queries[0].roleFilter, "all"); assert.equal(queries[0].query, "");
  assert.equal(queries[0].pageSize, 50); assert.equal(queries[0].cursor, null);
  pending.resolve({ counts: [{ agentKind: "codex", loadedCount: 999, total: 999 }], errors: {} }); await settle();
  assert.equal(surface.navigation.page, "sessions");
  click(surface.navButton("总览")); await settle(); assert.equal(surface.value("codex"), "155");
  click(surface.navButton("会话")); await settle();
  assert.deepEqual(queries.at(-1)?.workspaceIds, ["home", "project", "unmounted"], "history navigation retains the selected scope");
  assert.deepEqual(queries.at(-1)?.agentKinds, ["codex"]);
  assert.equal(surface.navigation.sessionWorkspaceMode, "all");
});

test("parent visibility cancels pending counts and rejects late results before a clean reactivation", async (t) => {
  const pending = deferred<AgentSessionCounts>();
  const surface = mountDashboard(t, { firstReply: pending }); await settle();
  const request = countRequests[0];
  surface.props.active = false; await settle();
  assert.ok(cancellations.some((item) => item.consumerId === request.consumerId && item.requestId === request.requestId));
  assert.equal(surface.historyButton("codex").props["aria-busy"], undefined);
  assert.equal(surface.value("codex"), "—");
  pending.resolve({ counts: [{ agentKind: "codex", loadedCount: 999, total: 999 }], errors: {} }); await settle();
  assert.equal(surface.value("codex"), "—"); assert.equal(countRequests.length, 1);
  surface.props.active = true; await settle();
  assert.equal(countRequests.length, 2); assert.equal(surface.value("codex"), "155");
});

test("active-card navigation keeps its Agent without starting a history query", async (t) => {
  const surface = mountDashboard(t); await settle();
  const active = descendants(surface.root).find((item) => item.type === "button" && String(item.props["aria-label"] ?? "").startsWith("查看 Codex CLI 的 活跃会话"));
  assert.ok(active); click(active); await settle();
  assert.equal(surface.navigation.sessionView, "active"); assert.equal(surface.navigation.agentFilter, "codex");
  assert.equal(surface.navigation.sessionWorkspaceMode, "all"); assert.equal(queries.length, 0);
  assert.equal(countRequests.length, 1);
});
