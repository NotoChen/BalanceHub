import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test, { after, before, type TestContext } from "node:test";
import { compileScript, parse } from "@vue/compiler-sfc";
import { createPinia, disposePinia, setActivePinia, storeToRefs, type Pinia } from "pinia";
import { cloneVNode, createRenderer, defineComponent, effectScope, h, nextTick, reactive, type Component } from "vue";
import { createServer, transformWithEsbuild, type ViteDevServer } from "vite";
import type {
  AgentAssetCatalog, AgentCatalogAction, AgentCatalogAsset, AgentCatalogBinding,
  AgentCatalogDefinition, AgentCatalogPlan, AgentCatalogSaveRequest, AgentCatalogUnresolvedTarget,
} from "../src/stores/agent-catalog-types.ts";
import type { AgentAssetCategory, AgentCliKind } from "../src/stores/provider-types.ts";
import type { AgentWorkspacePage } from "../src/stores/agent-workspace.ts";
import { assetAction } from "./agent-asset-fixtures.ts";
import { catalogAgentPanel, catalogAsset, catalogBinding, catalogObservations, catalogOperation, catalogPlan, catalogRelationPreview, catalogSnapshot, catalogTarget, workspaceAgent, workspaceHook } from "./agent-workspace-fixtures.ts";

type CatalogStoreModule = typeof import("../src/stores/agent-catalog.ts");
type EnvironmentModule = typeof import("../src/stores/agent-environment.ts");
type NavigationModule = typeof import("../src/stores/agent-workspace.ts");
type ConsoleModule = typeof import("../src/composables/useAgentCatalogConsole.ts");
type ViewModule = typeof import("../src/composables/useAgentCatalogView.ts");
type EditorModule = typeof import("../src/composables/useAgentDefinitionEditor.ts");
type Deferred = { promise: Promise<unknown>; resolve: (value: unknown) => void; reject: (error: unknown) => void };
type HostNode = { type: string; props: Record<string, unknown>; style: Record<string, string>; text: string; children: HostNode[]; parent: HostNode | null;
  dataset: { catalogFocus: unknown }; querySelectorAll: (selector: string) => HostNode[]; scrollIntoView: () => void; focus: () => void };

const sourceRoot = fileURLToPath(new URL("../src/", import.meta.url));
const agents = [workspaceAgent("claudeCode"), workspaceAgent("gemini"), workspaceAgent("codex"), workspaceAgent("grok")];
const calls: { command: string; args: Record<string, unknown> }[] = [];
const queues = new Map<string, Deferred[]>();
const unexpected: string[] = [];
const originalTimeout = globalThis.setTimeout;
let server: ViteDevServer;
let catalogs: CatalogStoreModule;
let environments: EnvironmentModule;
let navigations: NavigationModule;
let consoles: ConsoleModule;
let views: ViewModule;
let editors: EditorModule;
let backendCatalog: AgentAssetCatalog;
const components = new Map<string, Component>();

before(async () => {
  Object.assign(globalThis, { ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } });
  const windowEvents = new EventTarget();
  const documentEvents = new EventTarget();
  Object.assign(globalThis, { addEventListener: windowEvents.addEventListener.bind(windowEvents), removeEventListener: windowEvents.removeEventListener.bind(windowEvents), document: { documentElement: { style: {} }, visibilityState: "visible", addEventListener: documentEvents.addEventListener.bind(documentEvents), removeEventListener: documentEvents.removeEventListener.bind(documentEvents) } });
  Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", { configurable: true, value: {
    transformCallback() { return 1; }, unregisterCallback() {},
    invoke(command: string, args: Record<string, unknown> = {}) {
      // Verify transport metadata separately from business request assertions.
      const { progress, requestId, ...payload } = args;
      if (progress !== undefined) assert.equal(typeof (progress as { id: unknown }).id, "number");
      if (requestId !== undefined) assert.equal(typeof requestId, "string");
      calls.push({ command, args: payload });
      const response = queues.get(command)?.shift();
      if (response) return response.promise;
      if (command === "get_agent_catalog_revision") return Promise.resolve(backendCatalog.revision);
      if (command === "get_agent_asset_catalog") return Promise.resolve(structuredClone(backendCatalog));
      if (command === "cancel_agent_catalog_read") return Promise.resolve();
      if (command === "list_agent_catalog_operations") return Promise.resolve([]);
      unexpected.push(command);
      return Promise.reject(new Error(`Unexpected Tauri command: ${command}`));
    },
  } });
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    configFile: false, server: { middlewareMode: true, hmr: false }, appType: "custom", logLevel: "silent",
    resolve: { alias: [
      { find: /^@arco-design\/web-vue$/, replacement: "virtual:hook-catalog-messages" },
      { find: /^@arco-design\/web-vue\/es\/icon$/, replacement: "virtual:hook-catalog-icons" },
    ] },
    plugins: [{
      name: "hook-catalog-components",
      resolveId(id) { if (id.startsWith("virtual:hook-catalog-")) return `\0${id.slice("virtual:".length)}`; },
      async load(id) {
        if (id === "\0hook-catalog-messages") return "export const Message = { success() {}, error() {}, info() {}, warning() {} };";
        if (id === "\0hook-catalog-icons") return ["IconCommand", "IconCheckCircle", "IconDelete", "IconMore", "IconRefresh", "IconSettings", "IconClockCircle", "IconLoading", "IconApps", "IconBook", "IconBranch", "IconCode", "IconDashboard", "IconHistory", "IconLink", "IconThunderbolt"]
          .map((name) => `export const ${name} = { render() { return null; } };`).join("\n");
        // These tests exercise form state, not CodeMirror's browser layout.
        if (id.endsWith("/src/components/CodeEditor.vue")) return `import { h } from 'vue'; export default { props: ['modelValue', 'readonly', 'label'], emits: ['update:modelValue'], setup(props, { emit }) { return () => h('textarea', { value: props.modelValue, readonly: props.readonly, 'aria-label': props.label, onInput: event => emit('update:modelValue', event.target.value) }); } };`;
        if (!id.startsWith(sourceRoot) || !id.endsWith(".vue")) return;
        const { descriptor, errors } = parse(readFileSync(id, "utf8"), { filename: id });
        assert.deepEqual(errors, []);
        const compiled = compileScript(descriptor, { fs: { fileExists: existsSync, readFile: (path) => readFileSync(path, "utf8") }, id, inlineTemplate: true, templateOptions: { compilerOptions: { hoistStatic: false } } });
        return (await transformWithEsbuild(compiled.content, `${id}.ts`, { loader: "ts", target: "esnext" })).code;
      },
    }],
  });
  [catalogs, environments, navigations, consoles, views, editors] = await Promise.all([
    server.ssrLoadModule("/src/stores/agent-catalog.ts"), server.ssrLoadModule("/src/stores/agent-environment.ts"),
    server.ssrLoadModule("/src/stores/agent-workspace.ts"), server.ssrLoadModule("/src/composables/useAgentCatalogConsole.ts"),
    server.ssrLoadModule("/src/composables/useAgentCatalogView.ts"), server.ssrLoadModule("/src/composables/useAgentDefinitionEditor.ts"),
  ]) as [CatalogStoreModule, EnvironmentModule, NavigationModule, ConsoleModule, ViewModule, EditorModule];
  for (const name of ["AgentHookPanel", "AgentCatalogPanel", "AgentCatalogDetailDrawer", "AgentCatalogDefinitionModal", "AgentCatalogPlanModal", "AgentCatalogRelationModal"]) {
    components.set(name, (await server.ssrLoadModule(`/src/components/agent-workspace/${name}.vue`)).default as Component);
  }
});
after(async () => { await server?.close(); });

function pending(command: string): Deferred {
  let resolve!: Deferred["resolve"];
  let reject!: Deferred["reject"];
  const promise = new Promise<unknown>((done, fail) => { resolve = done; reject = fail; });
  const response = { promise, resolve, reject };
  queues.set(command, [...(queues.get(command) ?? []), response]);
  return response;
}
async function settle() { for (let index = 0; index < 16; index += 1) await Promise.resolve(); await nextTick(); }
async function until(predicate: () => boolean) {
  const deadline = Date.now() + 1_500;
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

function hookBinding(kind: AgentCliKind, id = `binding-${kind}`): AgentCatalogBinding {
  const binding = catalogBinding(kind, id);
  return { ...binding, canAdopt: true, actions: [assetAction("disable"), assetAction("remove")], native: { ...binding.native,
    category: "hook", label: "格式检查", effectiveState: "enabled", declaredState: "enabled",
    details: { kind: "hook", managed: false, enabled: "enabled", ruleCount: 1 },
    actions: [assetAction("inspect"), assetAction("preview"), assetAction("disable", { available: false, reason: "mutationDisabled" })],
  } };
}
function suspendedTarget(fields: Partial<AgentCatalogUnresolvedTarget> = {}): AgentCatalogUnresolvedTarget {
  return { targetId: "hook-target-codex", contextId: "context-codex", agentKind: "codex", scope: "user",
    drift: "inSync", appliedVersion: 2, state: "suspended", message: "此规则已从原生配置停用，可恢复已保留的定义。",
    actions: [assetAction("enable"), assetAction("remove")], ...fields };
}
function hookAsset(fields: Partial<AgentCatalogAsset> = {}): AgentCatalogAsset {
  const bindings = fields.bindings ?? [hookBinding("claudeCode"), hookBinding("gemini")];
  return catalogAsset("global-hook", { name: "格式检查", category: "hook", version: 2,
    application: { available: true, sourceBindingId: null, targets: hookTargets(), observations: catalogObservations([
      ...bindings.map((binding) => binding.native.agentKind), ...(fields.unresolvedTargets ?? []).map((target) => target.agentKind),
    ]), reason: null },
    bindings,
    variants: [{ id: "hook-variants", label: "原生规则", complete: true, summary: ["Claude Code / Gemini CLI"] }],
    ...fields,
  });
}
function hookTargets() {
  return agents.map((agent) => catalogTarget(agent.kind, { id: `hook-target-${agent.kind}`,
    label: `${agent.label} 用户 Hook`, categories: ["hook"] }));
}
function hookSnapshot(assets = [hookAsset()]): AgentAssetCatalog {
  const snapshot = catalogSnapshot({ assets, creatableCategories: ["skill", "mcp", "hook"],
    targets: hookTargets(),
  });
  snapshot.inventory.sources = snapshot.inventory.sources.map((source) => ({ ...source, categories: ["hook"] }));
  snapshot.inventory.hookRuleCounts = [{ agentKind: "claudeCode", ruleCount: 1 }, { agentKind: "gemini", ruleCount: 1 }, { agentKind: "codex", ruleCount: 0 }];
  return snapshot;
}
function hookDefinition(fields: Partial<AgentCatalogDefinition> = {}): AgentCatalogDefinition {
  return { assetId: "global-hook", name: "格式检查", category: "hook", version: 2, mcp: null, skillMarkdown: null,
    hook: { variants: [
      { agentKind: "claudeCode", event: "PostToolUse", groupJson: '{"matcher":"Write|Edit","hooks":[{"type":"command","command":"fixture-check","timeout":7}],"nativeOption":"keep"}' },
      { agentKind: "gemini", event: "AfterTool", groupJson: '{"matcher":"write_file","hooks":[{"name":"format","type":"command","command":"fixture-check","timeout":7000}]}' },
    ] }, files: [], notes: [], ...fields };
}
function hookPlan(action: AgentCatalogAction = "removeBinding", targetId = "binding-claudeCode", fields: Partial<AgentCatalogPlan> = {}): AgentCatalogPlan {
  const agentKind: AgentCliKind = targetId === "hook-target-codex" ? "codex" : targetId === "other-gemini" ? "gemini" : "claudeCode";
  return catalogPlan({ assetId: "global-hook", action, version: 2,
    targets: [{ targetId, label: "格式检查 · 用户 Hook", agentKind, contextId: `context-${agentKind}`, scope: "user", targetKind: targetId === "hook-target-codex" ? "retained" : "binding", available: true, reason: null,
      affectedAssetIds: targetId === "hook-target-codex" ? [] : [`native-${targetId}`], changes: [{ label: "修改选定规则", path: `/fixture/${agentKind}/config.json`, before: "规则存在", after: "规则已移除" }] }],
    ...fields,
  });
}

function fresh(t: TestContext, snapshot = hookSnapshot()) {
  calls.length = 0; unexpected.length = 0; queues.clear(); backendCatalog = snapshot;
  const pinia = createPinia(); setActivePinia(pinia);
  const catalog = catalogs.useAgentCatalogStore();
  const environment = environments.useAgentEnvironmentStore();
  const navigation = navigations.useAgentWorkspaceStore();
  catalog.catalogs = { __native__: snapshot };
  environment.inventories.__native__ = snapshot.inventory;
  environment.inventoryState.__native__ = "ready";
  const cleanup: (() => void)[] = [];
  function ui<T>(setup: () => T) { const scope = effectScope(); cleanup.push(() => scope.stop()); return scope.run(setup)!; }
  function console() {
    const { workspacePath, navigationRevision } = storeToRefs(navigation);
    return ui(() => consoles.useAgentCatalogConsole({ workspace: workspacePath, navigationRevision }));
  }
  t.after(async () => { cleanup.forEach((stop) => stop()); disposePinia(pinia); await settle(); queues.clear(); assert.deepEqual(unexpected, []); });
  return { pinia, catalog, navigation, environment, ui, console };
}
function viewProps(snapshot: AgentAssetCatalog) {
  return reactive({ catalog: snapshot as AgentAssetCatalog | null, page: "hook" as AgentWorkspacePage,
    query: "", agentFilter: null as AgentCliKind | null, agents: [...agents], focusedNativeId: null as string | null });
}

function node(type: string, text = ""): HostNode {
  return { type, props: {}, style: {}, text, children: [], parent: null,
    get dataset() { return { catalogFocus: this.props["data-catalog-focus"] }; },
    querySelectorAll(selector) {
      assert.ok(["[data-catalog-focus]", "[data-global-asset-id]"].includes(selector));
      const attribute = selector.slice(1, -1);
      return this.children.flatMap(descendants).filter((item) => item.props[attribute] !== undefined);
    },
    scrollIntoView() { this.props["data-test-scrolled"] = true; }, focus() { this.props["data-test-focused"] = true; },
  };
}
function remove(element: HostNode) {
  if (element.parent) element.parent.children = element.parent.children.filter((child) => child !== element);
  element.parent = null;
}
function insert(element: HostNode, parent: HostNode, anchor: HostNode | null = null) {
  remove(element);
  const index = anchor ? parent.children.indexOf(anchor) : -1;
  parent.children.splice(index < 0 ? parent.children.length : index, 0, element); element.parent = parent;
}
const renderer = createRenderer<HostNode, HostNode>({
  patchProp(element, key, _old, value) { element.props[key] = value; }, insert, remove,
  createElement: (type) => node(type), createText: (text) => node("#text", text), createComment: (text) => node("#comment", text),
  setText(element, text) { element.text = text; },
  setElementText(element, text) { element.children.forEach((child) => { child.parent = null; }); element.children = []; element.text = text; },
  parentNode: (element) => element.parent,
  nextSibling(element) { return element.parent?.children[element.parent.children.indexOf(element) + 1] ?? null; },
  insertStaticContent(content, parent, anchor) { const element = node("#static", content.replace(/<[^>]*>/g, "")); insert(element, parent, anchor); return [element, element]; },
});
function descendants(element: HostNode): HostNode[] { return [element, ...element.children.flatMap(descendants)]; }
function textContent(element: HostNode): string { return element.type === "#comment" ? "" : element.text + element.children.map(textContent).join(""); }
function invoke(element: HostNode, event: string, value?: unknown) {
  if (element.props.disabled === true || element.props.disabled === "") return;
  const handler = element.props[event]; assert.equal(typeof handler, "function", `Expected ${event} on ${element.type}`);
  if (typeof handler === "function") handler(value);
}
function button(root: HostNode, label: string) {
  const matches = descendants(root).filter((element) => element.type === "button" && textContent(element) === label);
  assert.equal(matches.length, 1, `Expected one button: ${label}`); return matches[0];
}
function byAttribute(root: HostNode, name: string, value: unknown) {
  const matches = descendants(root).filter((element) => element.props[name] === value);
  assert.equal(matches.length, 1, `Expected one ${name}=${value}`); return matches[0];
}
function mount(t: TestContext, name: string, pinia: Pinia, props: () => Record<string, unknown>) {
  const component = components.get(name); assert.ok(component);
  const app = renderer.createApp(defineComponent({ setup() { return () => {
    const current = props();
    if (name === "AgentCatalogDetailDrawer" && !current.resource) current.resource = {
      reading: true, managementExpanded: false, sharedView: false, binding: null, editor: { visible: false },
      reader: { loading: false, error: "", content: null, reload() {} },
      management: { panel: null, loading: false, error: "" }, parents: [], children: [],
    };
    return h(component, current);
  }; } }));
  app.use(pinia);
  for (const [name, tag] of [["a-button", "button"], ["a-select", "select"], ["a-option", "option"], ["a-input", "input"], ["a-input-password", "input"], ["a-textarea", "textarea"], ["a-dropdown", "div"], ["a-doption", "button"], ["a-switch", "button"], ["a-tooltip", "span"]]) {
    app.component(name, defineComponent({ inheritAttrs: false, setup(_, { attrs, slots }) { return () => h(tag, attrs, [slots.icon?.(), slots.default?.(), slots.content?.()]); } }));
  }
  app.component("a-popover", defineComponent({ props: { popupVisible: Boolean }, emits: ["popupVisibleChange"], setup(props, { slots, emit }) {
    return () => h("span", {}, [
      ...(slots.default?.() ?? []).map((child) => cloneVNode(child, { onClick: () => emit("popupVisibleChange", !props.popupVisible) })),
      props.popupVisible ? slots.content?.() : null,
    ]);
  } }));
  for (const name of ["a-modal", "a-drawer"]) app.component(name, defineComponent({
    inheritAttrs: false, props: { visible: Boolean }, emits: ["cancel"],
    setup(props, { attrs, slots, emit }) { return () => props.visible ? h("section", { ...attrs, role: "dialog" }, [
      h("button", { "aria-label": "关闭", onClick: () => emit("cancel") }, "关闭"), slots.title?.(), slots.default?.(),
    ]) : null; },
  }));
  const root = node("root"); app.mount(root); t.after(() => app.unmount()); return root;
}

test("user Hooks share the global catalog row and events, with session integration folded below", async (t) => {
  const snapshot = hookSnapshot(); const context = fresh(t, snapshot); const events: unknown[] = [];
  const root = mount(t, "AgentHookPanel", context.pinia, () => ({
    catalog: snapshot, agents, agentFilter: null, query: "", focusedAssetId: "native-binding-gemini", loading: false, error: "", busy: () => false,
    center: { inventory: snapshot.inventory }, hooks: { inspectionFor: workspaceHook, isRowBusy: () => false, rowError: () => null },
    onCreate: (category: AgentAssetCategory) => events.push(category), onDetail: (id: string) => events.push(id),
    onAction: (...args: unknown[]) => events.push(args), onManage: (...args: unknown[]) => events.push(["manage", ...args]),
  }));
  const globalRow = byAttribute(root, "data-global-asset-id", "global-hook");
  assert.equal(globalRow.props["aria-current"], "true");
  assert.match(textContent(globalRow), /格式检查/);
  assert.equal(descendants(globalRow).filter((entry) => String(entry.props.class).includes("agent-catalog-agent-control")).length, 2);
  assert.equal(descendants(root).some((entry) => String(entry.props.class).includes("agent-hook-native-entry")), false);
  const integration = byAttribute(root, "aria-label", "BalanceHub 会话状态接入");
  assert.equal(integration.type, "details"); assert.ok(!integration.props.open);
  invoke(button(root, "新建 Hook"), "onClick");
  invoke(byAttribute(globalRow, "aria-label", "打开 格式检查 详情"), "onClick");
  const gemini = byAttribute(globalRow, "data-agent-kind", "gemini");
  assert.match(textContent(gemini), /gemini CLI.*启用/i);
  invoke(gemini, "onClick");
  invoke(byAttribute(globalRow, "aria-label", "将 格式检查 配置到 Agent"), "onClick");
  assert.deepEqual(events, ["hook", "global-hook", ["manage", "global-hook", "gemini"], ["global-hook", "applyDefinition", undefined]]);
  assert.equal(calls.length, 0);
});

test("catalog panel opens a three-file plan again after cancellation without a duplicate close or writes", async (t) => {
  const source = catalogBinding("codex", "transfer-source"); source.native.category = "skill";
  const target = catalogTarget("claudeCode");
  const asset = catalogAsset("transfer-skill", { category: "skill", ownership: "observed", version: null, bindings: [source],
    application: { available: true, sourceBindingId: source.id, targets: [target], observations: catalogObservations(["codex"]), reason: null } });
  const snapshot = catalogSnapshot({ assets: [asset] }); const context = fresh(t, snapshot); const controller = context.console();
  const events: unknown[] = [];
  let planning: ReturnType<typeof controller.openPlan>;
  const list = mount(t, "AgentCatalogPanel", context.pinia, () => ({ catalog: controller.catalog.value, page: "skill", query: "", agentFilter: null,
    agents, loading: false, error: "", busy: () => false, agentSelection: controller.management.selection.value,
    agentPanel: controller.management.panel.value, agentLoading: controller.management.loading.value, agentError: controller.management.error.value,
    onAction: (id: string, action: AgentCatalogAction, targets: string[]) => { events.push(["action", action, targets]); planning = controller.openPlan(id, action, targets); },
  }));
  const modal = mount(t, "AgentCatalogPlanModal", context.pinia, () => ({ visible: controller.planVisible.value, name: controller.planName.value,
    catalog: controller.catalog.value, action: controller.planAction.value, choices: controller.targetChoices.value, selected: controller.selectedTargets.value,
    plan: controller.pendingPlan.value, preparing: controller.preparing.value, error: controller.planError.value, expired: controller.planExpired.value,
    canConfirm: controller.canConfirm.value, onClose: controller.closePlan }));
  for (let cycle = 0; cycle < 2; cycle += 1) {
    const discovery = pending("plan_agent_catalog");
    invoke(byAttribute(list, "aria-label", `将 ${asset.name} 配置到 Agent`), "onClick");
    assert.equal(controller.management.selection.value, null); assert.equal(controller.planVisible.value, true);
    const plan = catalogPlan({ assetId: asset.id, token: "reopen-token-" + cycle, planId: "reopen-plan-" + cycle,
      definitionChange: { kind: "adopt", name: asset.name, beforeVersion: null, afterVersion: 1 },
      targets: [{ ...catalogPlan().targets[0], targetId: target.id, agentKind: "claudeCode", contextId: target.contextId,
        changes: ["SKILL.md", "scripts/check.sh", "references/guide.md"].map((path) => ({ label: path, path: "/fixture/claude/skills/transfer/" + path, before: null, after: "fixture content" })) }] });
    discovery.resolve({ ...plan, token: null, planId: null }); await planning;
    controller.setTargets([target.id]);
    const selected = pending("plan_agent_catalog"); const previewing = controller.preparePlan();
    selected.resolve(plan); await previewing; await nextTick();
    assert.equal(controller.canConfirm.value, true); assert.deepEqual(controller.selectedTargets.value, [target.id]);
    assert.ok(byAttribute(modal, "role", "dialog")); assert.match(textContent(modal), /SKILL\.md/); assert.match(textContent(modal), /scripts\/check\.sh/); assert.match(textContent(modal), /references\/guide\.md/);
    invoke(button(modal, "取消"), "onClick"); await nextTick();
    assert.equal(controller.planVisible.value, false); assert.equal(controller.pendingPlan.value, null);
    assert.equal(controller.preparing.value, false); assert.equal(controller.planError.value, "");
    assert.equal(descendants(modal).some((item) => item.props.role === "dialog"), false);
  }
  assert.deepEqual(events, [["action", "applyDefinition", undefined], ["action", "applyDefinition", undefined]]);
  assert.equal(calls.some((call) => ["save_agent_catalog_definition", "adopt_agent_catalog_asset", "apply_agent_catalog"].includes(call.command)), false);
});

test("suspended Hook targets remain in the same filtered global row without pretending to be native rules", (t) => {
  const asset = hookAsset({ bindings: [], unresolvedTargets: [suspendedTarget(), suspendedTarget({ targetId: "unknown-claude", agentKind: "claudeCode", state: "unknown", drift: "unknown", actions: [] })] });
  const snapshot = hookSnapshot([asset]); const context = fresh(t, snapshot); const props = viewProps(snapshot);
  const view = context.ui(() => views.useAgentCatalogView(props));
  props.agentFilter = "codex"; view.state.value = "disabled";
  assert.deepEqual(view.rows.value.map((row) => row.id), [asset.id]);
  assert.deepEqual(view.bindingsFor(asset), []); assert.deepEqual(view.unresolvedFor(asset).map((target) => target.targetId), ["hook-target-codex"]);
  assert.equal(snapshot.inventory.assets.length, 0);
  props.query = "定义已保留"; assert.equal(view.rows.value.length, 1);
  view.state.value = "enabled"; assert.equal(view.rows.value.length, 0);
  view.state.value = "all"; props.query = ""; props.agentFilter = null;
  view.provision.value = "independent"; assert.equal(view.rows.value.length, 0);
  view.clearFilters(); props.focusedNativeId = "absent-native"; assert.equal(view.rows.value.length, 1);
});

test("Hook discovery uncertainty remains in a collapsed summary and plugin empty state follows creatable categories", (t) => {
  const snapshot = hookSnapshot([]); snapshot.inventory.hookRuleCounts = [{ agentKind: "claudeCode", ruleCount: null }];
  const context = fresh(t, snapshot); const props = viewProps(snapshot);
  const root = mount(t, "AgentCatalogPanel", context.pinia, () => ({ ...props, loading: false, error: "", busy: () => false }));
  assert.match(textContent(root), /Hook 规则数量尚未确认/);
  const notes = byAttribute(root, "aria-label", "扫描说明");
  assert.equal(notes.type, "details"); assert.ok(!notes.props.open);
  assert.ok(button(root, "新建 Hook"));
  const plugin = mount(t, "AgentCatalogPanel", context.pinia, () => ({ ...props, page: "extension", loading: false, error: "", busy: () => false }));
  assert.match(textContent(plugin), /当前范围暂无 插件与扩展/);
  assert.doesNotMatch(textContent(plugin), /新建 Hook/);
});

test("Hook creation starts with the selected Agent and never converts or duplicates a native variant", async (t) => {
  const context = fresh(t); const saved: AgentCatalogSaveRequest[] = [];
  const props = reactive({ visible: true, definition: null, editingAssetId: null, category: "hook" as const,
    loading: false, saving: false, agents, preferredAgentKind: "gemini" as AgentCliKind });
  const editor = context.ui(() => editors.useAgentDefinitionEditor(props, (request) => saved.push(request)));
  assert.equal(editor.activeHookAgent.value, "gemini");
  editor.draft.name = "自定义检查"; editor.draft.hookVariants[0].event = "AfterTool"; editor.draft.hookVariants[0].groupJson = '{"hooks":[{"type":"command","command":"fixture-check"}]}';
  editor.addHookVariant("claudeCode"); editor.addHookVariant("claudeCode");
  assert.equal(editor.draft.hookVariants.length, 2);
  assert.deepEqual(editor.activeHookVariant.value, { agentKind: "claudeCode", event: "", groupJson: "" });
  editor.save(); assert.equal(saved.length, 0); assert.match(editor.validationError.value, /原生事件/);
  editor.activeHookVariant.value!.event = "PostToolUse"; editor.activeHookVariant.value!.groupJson = '{"matcher":"Write","hooks":[{"type":"command","command":"fixture-claude-check"}]}';
  editor.save();
  assert.deepEqual(saved[0].hook?.variants.map((variant) => [variant.agentKind, variant.event]), [["gemini", "AfterTool"], ["claudeCode", "PostToolUse"]]);
  assert.equal(saved[0].mcp, null); assert.equal(saved[0].skillMarkdown, null); assert.equal(saved[0].assetId, null);
  editor.draft.hookVariants[0].event = "ChangedAfterSave";
  assert.equal(saved[0].hook?.variants[0].event, "AfterTool");
  assert.equal(calls.length, 0);
});

test("Hook editing preserves sibling variants, opaque secret markers and native JSON unchanged", (t) => {
  const context = fresh(t); const definition = hookDefinition(); const original = structuredClone(definition);
  const saved: AgentCatalogSaveRequest[] = [];
  const props = reactive({ visible: true, definition, editingAssetId: definition.assetId, category: definition.category, loading: false, saving: false, agents, preferredAgentKind: "gemini" as AgentCliKind });
  const editor = context.ui(() => editors.useAgentDefinitionEditor(props, (request) => saved.push(request)));
  assert.equal(editor.activeHookAgent.value, "gemini");
  editor.activeHookVariant.value!.groupJson = editor.activeHookVariant.value!.groupJson.replace("fixture-check", "fixture-updated-check");
  editor.save();
  assert.deepEqual(saved[0].hook?.variants[0], original.hook?.variants[0]);
  assert.match(saved[0].hook!.variants[0].groupJson, /nativeOption/);
  assert.match(saved[0].hook!.variants[1].groupJson, /fixture-updated-check/);
  assert.equal(saved[0].expectedVersion, 2); assert.deepEqual(definition, original);
  editor.removeHookVariant("gemini"); editor.removeHookVariant("claudeCode"); editor.save();
  assert.equal(saved.length, 1); assert.match(editor.validationError.value, /至少一个/);
});

test("the real Hook editor exposes native variants and remains closable while saving", async (t) => {
  const context = fresh(t); const definition = hookDefinition(); const events: string[] = [];
  const props = reactive({ visible: true, definition, editingAssetId: definition.assetId, category: definition.category,
    creatableCategories: ["hook"] as AgentAssetCategory[], loading: false, saving: false, error: "", agents, preferredAgentKind: "gemini" as AgentCliKind });
  const root = mount(t, "AgentCatalogDefinitionModal", context.pinia, () => ({ ...props, onClose: () => { events.push("close"); props.visible = false; } }));
  assert.equal(byAttribute(root, "id", "agent-hook-variant-gemini").props["aria-selected"], true);
  invoke(byAttribute(root, "id", "agent-hook-variant-claudeCode"), "onClick"); await nextTick();
  assert.equal(byAttribute(root, "aria-label", "Hook 原生事件名").props.modelValue, "PostToolUse");
  assert.equal(byAttribute(root, "aria-label", "Hook 原生规则 JSON").props.value, definition.hook?.variants[0].groupJson);
  props.saving = true; await nextTick();
  assert.ok(byAttribute(root, "role", "dialog").props["mask-closable"] !== false);
  invoke(byAttribute(root, "aria-label", "关闭"), "onClick"); await nextTick();
  assert.deepEqual(events, ["close"]); assert.equal(descendants(root).some((entry) => entry.props.role === "dialog"), false);
});

test("the editor exposes separate save-only and save-and-apply intents for the same complete draft", async (t) => {
  const context = fresh(t); const definition = hookDefinition(); const events: { kind: string; request: AgentCatalogSaveRequest }[] = [];
  const root = mount(t, "AgentCatalogDefinitionModal", context.pinia, () => ({ visible: true, definition, editingAssetId: definition.assetId,
    category: "hook", creatableCategories: ["hook"], loading: false, saving: false, error: "", agents, preferredAgentKind: "gemini",
    onSave: (request: AgentCatalogSaveRequest) => events.push({ kind: "save", request }), onApply: (request: AgentCatalogSaveRequest) => events.push({ kind: "apply", request }) }));
  assert.equal(button(root, "保存到共享库").props["html-type"], "submit");
  const form = descendants(root).find((item) => item.type === "form"); assert.ok(form);
  invoke(form, "onSubmit", { preventDefault() {} });
  invoke(button(root, "配置到 Agent…"), "onClick");
  assert.deepEqual(events.map((event) => event.kind), ["save", "apply"]);
  assert.deepEqual(events[0].request, events[1].request); assert.deepEqual(events[0].request.hook, definition.hook);
  assert.equal(calls.length, 0);
});

test("resource detail features focus source evidence and relationship entries preserve exact backend identities", async (t) => {
  const asset = hookAsset({ candidateIds: ["other-hook"], separatedAssetIds: ["separated-hook"], manualAssociations: [
    { id: "detachable", label: "手动整理的来源", sourceAssetIds: ["other-hook"], canDetach: true, reason: null },
    { id: "blocked", label: "不能完整还原的来源", sourceAssetIds: ["old-hook"], canDetach: false, reason: "来源已不完整" },
  ] });
  const snapshot = hookSnapshot([asset, hookAsset({ id: "other-hook" }), hookAsset({ id: "separated-hook" })]);
  const context = fresh(t, snapshot); const events: unknown[] = [];
  const detailProps = reactive({ visible: true, asset, catalog: snapshot, agents,
    focusedFeature: "provider-unknown", libraryBusy: {}, libraryErrors: {}, onRelation: (intent: unknown) => events.push(intent) });
  const root = mount(t, "AgentCatalogDetailDrawer", context.pinia, () => detailProps);
  await nextTick();
  const evidence = descendants(root).filter((item) => item.props["data-catalog-focus"] === "provider-unknown");
  assert.equal(evidence.length, 2); assert.equal(evidence[0].props["data-test-focused"], true);
  assert.match(textContent(evidence[0]), /用户/);
  detailProps.focusedFeature = "installation-configEntry"; await nextTick();
  assert.equal(descendants(root).find((item) => item.props["data-catalog-focus"] === "installation-configEntry")?.props["data-test-scrolled"], true);
  invoke(button(root, "比较内容"), "onClick"); invoke(button(root, "恢复提示"), "onClick");
  const detach = descendants(root).filter((item) => item.type === "button" && textContent(item) === "预览解除");
  assert.equal(detach.length, 2); assert.equal(detach[1].props.disabled, true);
  invoke(detach[0], "onClick"); invoke(detach[1], "onClick");
  assert.deepEqual(events, [
    { kind: "compare", leftAssetId: "global-hook", rightAssetId: "other-hook" },
    { kind: "restoreHint", leftAssetId: "global-hook", rightAssetId: "separated-hook" },
    { kind: "detach", associationId: "detachable" },
  ]);
  assert.match(textContent(root), /来源已不完整/); assert.equal(calls.length, 0);
});

test("the real relation dialog dismisses keep-apart without a mutation and exposes only backend-approved choices", async (t) => {
  const context = fresh(t); const preview = catalogRelationPreview(); const events: unknown[] = [];
  const props = reactive({ visible: true, preview, intent: { kind: "compare", leftAssetId: "global-asset", rightAssetId: "other-asset" },
    agents, loading: false, error: "", expired: false, canConfirm: false, canBack: false });
  const root = mount(t, "AgentCatalogRelationModal", context.pinia, () => ({ ...props,
    onClose: () => { props.visible = false; events.push("close"); }, onChoose: (capability: unknown) => events.push(capability), onConfirm: () => events.push("confirm") }));
  invoke(button(root, "关闭比较"), "onClick"); await nextTick();
  assert.deepEqual(events, ["close"]); assert.equal(descendants(root).some((item) => item.props.role === "dialog"), false);
  props.visible = true; await nextTick();
  invoke(button(root, "合并展示到资源 1"), "onClick");
  assert.equal(descendants(root).some(item => item.type === "button" && textContent(item) === "合并展示到资源 2"), false);
  assert.match(textContent(root), /合并展示到资源 2/);
  invoke(button(root, "不再提醒"), "onClick");
  assert.deepEqual(events.slice(1), [preview.capabilities[0], preview.capabilities[2]]);
  props.loading = true; await nextTick(); invoke(byAttribute(root, "aria-label", "关闭"), "onClick");
  assert.equal(events.at(-1), "close"); assert.equal(calls.length, 0);
});

test("an expired relation dialog rereads a fresh token without a back route or duplicate loading action", async (t) => {
  const context = fresh(t, catalogSnapshot()); const relations = context.console().relations;
  const intent = { kind: "restoreHint", leftAssetId: "global-asset", rightAssetId: "other-asset" } as const;
  const expired = catalogRelationPreview({ action: intent.kind, token: "expired-relation-token", expiresAt: new Date(Date.now() - 1_000).toISOString() });
  const response = pending("preview_agent_catalog_relation"); const opening = relations.open(intent);
  response.resolve(expired); await opening;
  let retrying: ReturnType<typeof relations.retry>;
  const root = mount(t, "AgentCatalogRelationModal", context.pinia, () => ({ visible: relations.visible.value, preview: relations.preview.value,
    intent: relations.intent.value, agents, loading: relations.loading.value, error: relations.error.value, expired: relations.expired.value,
    canConfirm: relations.canConfirm.value, canBack: relations.canBack.value, onClose: relations.close,
    onRetry: () => { retrying = relations.retry(); } }));
  assert.equal(relations.error.value, ""); assert.equal(relations.expired.value, true); assert.equal(relations.canBack.value, false);
  assert.equal(button(root, "确认恢复提示").props.disabled, true);
  const retry = pending("preview_agent_catalog_relation"); invoke(button(root, "重新读取整理预览"), "onClick"); await settle();
  assert.equal(relations.loading.value, true); assert.equal(relations.canConfirm.value, false);
  assert.match(textContent(root), /正在读取内容与整理条件/);
  assert.equal(descendants(root).some((item) => item.type === "button" && textContent(item) === "重新读取整理预览"), false);
  assert.deepEqual(calls.filter((call) => call.command === "preview_agent_catalog_relation").map((call) => call.args), [
    { request: { intent, expectedRevision: "catalog-revision", workspace: null } },
    { request: { intent, expectedRevision: "catalog-revision", workspace: null } },
  ]);
  retry.resolve({ ...expired, token: "fresh-relation-token", expiresAt: new Date(Date.now() + 300_000).toISOString() }); await retrying; await nextTick();
  assert.equal(relations.loading.value, false); assert.equal(relations.expired.value, false); assert.equal(relations.canConfirm.value, true);
  assert.equal(relations.preview.value?.token, "fresh-relation-token"); assert.equal(button(root, "确认恢复提示").props.disabled, false);
  assert.doesNotMatch(textContent(root), /整理预览已过期/);
  assert.equal(calls.some((call) => call.command === "commit_agent_catalog_relation"), false);
  invoke(button(root, "取消"), "onClick"); await nextTick(); assert.equal(relations.visible.value, false);
});

test("binding and suspended-target controls use catalog capabilities and exact opaque IDs", (t) => {
  const denied = hookBinding("gemini"); denied.actions = [assetAction("disable", { available: false, reason: "policyBlocked" })];
  denied.native.actions.push(assetAction("enable"));
  const asset = hookAsset({ bindings: [hookBinding("claudeCode"), denied], unresolvedTargets: [suspendedTarget()] });
  const snapshot = hookSnapshot([asset]); const context = fresh(t, snapshot); const actions: unknown[] = [];
  const action = (action: AgentCatalogAction, label: string, target: string, available = true) => ({
    action, label, targetIds: [target], available, reason: available ? null : "原生策略不允许停用",
    affectedAssetIds: [], parentNativeAssetId: null,
  });
  const resource = { reading: true, managementExpanded: true, sharedView: false, editor: { visible: false },
    reader: { loading: false, error: "", content: null }, parents: [], children: [],
    management: { loading: false, error: "", panel: { entries: [
      { targetId: "binding-claudeCode", actions: [action("disable", "停用", "binding-claudeCode"), action("removeBinding", "移除此配置", "binding-claudeCode")] },
      { targetId: "binding-gemini", actions: [action("disable", "停用", "binding-gemini", false)] },
      { targetId: "hook-target-codex", actions: [action("enable", "启用", "hook-target-codex"), action("removeBinding", "移除此配置", "hook-target-codex"), action("applyDefinition", "更新配置", "hook-target-codex")] },
    ] } },
  };
  const root = mount(t, "AgentCatalogDetailDrawer", context.pinia, () => ({ visible: true, asset, catalog: snapshot, agents, resource,
    libraryBusy: {}, libraryErrors: {}, onAction: (...args: unknown[]) => actions.push(args) }));
  const claude = byAttribute(root, "data-binding-id", "binding-claudeCode");
  invoke(button(claude, "停用"), "onClick"); invoke(button(claude, "移除此配置"), "onClick");
  const gemini = byAttribute(root, "data-binding-id", "binding-gemini");
  assert.equal(descendants(gemini).some(item => item.type === "button" && textContent(item) === "停用"), false);
  assert.match(textContent(gemini), /原生策略不允许停用/);
  assert.equal(descendants(gemini).some((entry) => entry.type === "button" && textContent(entry) === "启用"), false);
  const suspended = byAttribute(root, "data-target-id", "hook-target-codex");
  assert.match(textContent(suspended), /已停用，定义已保留/);
  invoke(button(suspended, "启用"), "onClick"); invoke(button(suspended, "移除此配置"), "onClick");
  invoke(button(suspended, "更新配置"), "onClick");
  assert.deepEqual(actions, [["disable", ["binding-claudeCode"]], ["removeBinding", ["binding-claudeCode"]],
    ["enable", ["hook-target-codex"]], ["removeBinding", ["hook-target-codex"]], ["applyDefinition", ["hook-target-codex"]]]);
});

test("a user-created Hook can be adopted into the existing shared editor without applying native changes", async (t) => {
  const observed = hookAsset({ ownership: "observed", version: null,
    application: { available: true, sourceBindingId: "binding-claudeCode", targets: hookTargets(), observations: catalogObservations(["claudeCode"]), reason: null }, bindings: [hookBinding("claudeCode")] });
  const context = fresh(t, hookSnapshot([observed])); const controller = context.console();
  controller.openDetail(observed.id);
  const response = pending("adopt_agent_catalog_asset"); const adopting = controller.adoptBinding("binding-claudeCode");
  assert.equal(controller.libraryBusy.value["binding-claudeCode"], true);
  backendCatalog = hookSnapshot(); response.resolve(hookDefinition()); await adopting;
  assert.equal(controller.libraryBusy.value["binding-claudeCode"], false);
  assert.equal(controller.selectedAsset.value?.ownership, "managed");
  assert.deepEqual(calls.find((call) => call.command === "adopt_agent_catalog_asset")?.args, { request: {
    bindingId: "binding-claudeCode", expectedRevision: "catalog-revision", workspace: null,
  } });
  const load = pending("get_agent_catalog_definition"); const opening = controller.openEditor("global-hook", "hook");
  load.resolve(hookDefinition()); await opening;
  assert.equal(controller.editorDefinition.value?.hook?.variants[0].event, "PostToolUse");
  assert.equal(calls.some((call) => call.command === "apply_agent_catalog"), false);
});

async function prepare(controller: ReturnType<ConsoleModule["useAgentCatalogConsole"]>, plan: AgentCatalogPlan) {
  const discovery = pending("plan_agent_catalog"); const opening = controller.openPlan(plan.assetId, plan.action);
  discovery.resolve({ ...plan, token: null, planId: null }); await opening;
  controller.setTargets(plan.targets.filter((target) => target.available).map((target) => target.targetId));
  if (!controller.selectedTargets.value.length) return;
  const response = pending("plan_agent_catalog"); const preparing = controller.preparePlan(); response.resolve(plan); await preparing;
}

test("Hook action choices include recoverable receipts and keep native direct capabilities separate", async (t) => {
  const snapshot = hookSnapshot([hookAsset({ unresolvedTargets: [suspendedTarget()] })]);
  const context = fresh(t, snapshot); const controller = context.console();
  const disabled = hookPlan("disable");
  disabled.targets.push({ ...hookPlan("disable", "hook-target-codex").targets[0], available: false, reason: "此范围已经停用" });
  const discovery = pending("plan_agent_catalog"); const opening = controller.openPlan("global-hook", "disable");
  assert.deepEqual(controller.targetChoices.value, []);
  discovery.resolve({ ...disabled, token: null, planId: null }); await opening;
  assert.equal(controller.targetChoices.value.find((choice) => choice.id === "binding-claudeCode")?.available, true);
  assert.equal(controller.targetChoices.value.find((choice) => choice.id === "hook-target-codex")?.available, false);
  await prepare(controller, hookPlan("enable", "hook-target-codex"));
  assert.equal(controller.targetChoices.value.find((choice) => choice.id === "hook-target-codex")?.available, true);
  assert.deepEqual(calls.filter((call) => call.command === "plan_agent_catalog").at(-1)?.args, { request: {
    source: { kind: "catalog", assetId: "global-hook", expectedVersion: 2 }, action: "enable", targetIds: ["hook-target-codex"], expectedRevision: snapshot.revision, workspace: null,
  } });
  assert.equal(controller.canConfirm.value, true);
});

test("backend missing-variant decisions prevent apply without frontend schema guesses", async (t) => {
  const context = fresh(t); const controller = context.console();
  const plan = hookPlan("applyDefinition", "hook-target-codex");
  plan.targets[0] = { ...plan.targets[0], agentKind: "codex", available: false, reason: "请先添加 Codex 的原生定义" };
  await prepare(controller, plan);
  const root = mount(t, "AgentCatalogPlanModal", context.pinia, () => ({ visible: controller.planVisible.value, name: controller.planName.value, catalog: controller.catalog.value,
    action: controller.planAction.value, choices: controller.targetChoices.value, selected: controller.selectedTargets.value,
    plan: controller.pendingPlan.value, preparing: controller.preparing.value, error: controller.planError.value,
    expired: controller.planExpired.value, canConfirm: controller.canConfirm.value, onConfirm: controller.confirmPlan, onClose: controller.closePlan }));
  assert.match(textContent(root), /请先添加 Codex 的原生定义/);
  assert.equal(descendants(root).some((item) => item.type === "button" && textContent(item) === "确认写入"), false);
  assert.equal(button(root, "预览更改").props.disabled, true);
  assert.equal(calls.some((call) => call.command === "apply_agent_catalog"), false);
});

test("Hook plans disclose backend-reported effects on other native bindings and their sources", (t) => {
  const secondary = hookBinding("gemini", "plugin-provided-hook"); secondary.native.label = "插件文件检查";
  secondary.native.path = "/fixture/gemini/extensions/check/hooks/hooks.json";
  const snapshot = hookSnapshot([hookAsset(), hookAsset({ id: "plugin-hook", name: "插件文件检查", bindings: [secondary] })]);
  const context = fresh(t, snapshot); const plan = hookPlan("disable");
  plan.targets[0].affectedAssetIds.push(secondary.native.stableId);
  const root = mount(t, "AgentCatalogPlanModal", context.pinia, () => ({ visible: true, name: snapshot.assets[0].name, catalog: snapshot,
    action: "disable", choices: [], selected: ["binding-claudeCode"], plan, preparing: false, error: "", expired: false, canConfirm: true }));
  assert.match(textContent(root), /影响 2 个来源入口/);
  assert.match(textContent(root), /插件文件检查/);
  assert.match(textContent(root), /\/fixture\/gemini\/extensions\/check\/hooks\/hooks.json/);
  assert.match(textContent(root), /\/fixture\/claudeCode\/config.json/);
});

test("confirming Hook removal closes its actual modal before pending apply and keeps navigation usable", async (t) => {
  const context = fresh(t); const controller = context.console(); await prepare(controller, hookPlan());
  const root = mount(t, "AgentCatalogPlanModal", context.pinia, () => ({ visible: controller.planVisible.value, name: controller.planName.value, catalog: controller.catalog.value,
    action: controller.planAction.value, choices: controller.targetChoices.value, selected: controller.selectedTargets.value,
    plan: controller.pendingPlan.value, preparing: controller.preparing.value, error: controller.planError.value,
    expired: controller.planExpired.value, canConfirm: controller.canConfirm.value, onConfirm: controller.confirmPlan, onClose: controller.closePlan }));
  assert.match(textContent(root), /共享库中的定义保留/);
  const apply = pending("apply_agent_catalog"); invoke(button(root, "确认移除"), "onClick"); await nextTick();
  assert.equal(controller.planVisible.value, false); assert.equal(context.catalog.starting["global-hook"], true);
  assert.equal(descendants(root).some((entry) => entry.props.role === "dialog"), false);
  context.navigation.openPage("mcp", "gemini"); assert.equal(context.navigation.page, "mcp");
  apply.reject({ kind: "sourceConflict", message: "原生规则已被其他程序修改" }); await settle();
  assert.equal(context.catalog.starting["global-hook"], false); assert.match(context.catalog.startErrors["global-hook"], /已被其他程序修改/);
  assert.equal(controller.planError.value, ""); assert.equal(controller.planVisible.value, false);
  assert.deepEqual(calls.find((call) => call.command === "apply_agent_catalog")?.args, { request: { planToken: "catalog-plan", assetId: "global-hook", action: "removeBinding" } });
});

test("Hook restore timeout releases start state, does not replay writes and ignores a late response", async (t) => {
  const context = fresh(t, hookSnapshot([hookAsset({ unresolvedTargets: [suspendedTarget()] })]));
  const controller = context.console(); await prepare(controller, hookPlan("enable", "hook-target-codex"));
  await fastTimeout(15_000, async () => {
    const apply = pending("apply_agent_catalog"); controller.confirmPlan();
    context.navigation.selectWorkspace("/fixture/another-project");
    await until(() => !context.catalog.starting["global-hook"]);
    assert.match(context.catalog.startErrors["global-hook"], /不会自动重试/);
    apply.resolve(catalogOperation({ assetId: "global-hook", action: "enable" })); await settle();
    assert.deepEqual(Object.keys(context.catalog.operations), []);
    assert.equal(calls.filter((call) => call.command === "apply_agent_catalog").length, 1);
    assert.equal(controller.pendingPlan.value, null); assert.equal(controller.planVisible.value, false);
  });
});

test("a stale Hook plan cannot replace the current page or another action after navigation", async (t) => {
  const context = fresh(t); const controller = context.console();
  const response = pending("plan_agent_catalog"); const preparing = controller.openPlan("global-hook", "removeBinding");
  context.navigation.openPage("sessions", "codex");
  assert.equal(controller.preparing.value, false); assert.equal(controller.planVisible.value, false);
  response.resolve(hookPlan()); await preparing;
  assert.equal(controller.pendingPlan.value, null); assert.equal(controller.planError.value, "");
});

test("saving Hook variants never applies them and a late save cannot reopen a new workspace editor", async (t) => {
  const context = fresh(t); const controller = context.console();
  const definition = hookDefinition(); const load = pending("get_agent_catalog_definition");
  const opening = controller.openEditor("global-hook", "hook"); load.resolve(definition); await opening;
  const save = pending("save_agent_catalog_definition");
  const request: AgentCatalogSaveRequest = { assetId: definition.assetId, expectedVersion: 2, name: definition.name, category: "hook",
    mcp: null, skillMarkdown: null, hook: structuredClone(definition.hook) };
  const saving = controller.saveDefinition(request);
  assert.equal(controller.editorSaving.value, true); controller.closeEditor();
  context.navigation.selectWorkspace("/fixture/new-project");
  await controller.openEditor(null, "hook");
  save.resolve(hookDefinition({ version: 3 })); await saving;
  assert.equal(controller.editorVisible.value, true); assert.equal(controller.editorAssetId.value, null);
  assert.equal(controller.editorDefinition.value, null); assert.equal(controller.editorSaving.value, false);
  assert.equal(calls.some((call) => call.command === "apply_agent_catalog"), false);
  assert.deepEqual(calls.find((call) => call.command === "get_agent_asset_catalog")?.args, { workspace: null });
});

test("a Hook save timeout leaves the editor cancellable and late completion cannot mutate its draft", async (t) => {
  const context = fresh(t); const controller = context.console(); await controller.openEditor(null, "hook");
  await fastTimeout(30_000, async () => {
    const response = pending("save_agent_catalog_definition");
    await controller.saveDefinition({ assetId: null, expectedVersion: null, name: "自定义 Hook", category: "hook", mcp: null, skillMarkdown: null, hook: hookDefinition().hook });
    assert.equal(controller.editorSaving.value, false); assert.match(controller.editorError.value, /超时/);
    controller.closeEditor(); response.resolve(hookDefinition()); await settle();
    assert.equal(controller.editorVisible.value, false); assert.equal(controller.editorDefinition.value, null);
    assert.equal(calls.some((call) => call.command === "apply_agent_catalog"), false);
  });
});

type LibraryMutation = "save" | "adopt";
async function beginLibraryMutation(controller: ReturnType<ConsoleModule["useAgentCatalogConsole"]>, mutation: LibraryMutation) {
  controller.openDetail("global-hook");
  if (mutation === "save") {
    const definition = hookDefinition(); const load = pending("get_agent_catalog_definition");
    const opening = controller.openEditor("global-hook", "hook"); load.resolve(definition); await opening;
    const response = pending("save_agent_catalog_definition");
    const completion = controller.saveDefinition({ assetId: definition.assetId, expectedVersion: definition.version,
      name: definition.name, category: definition.category, mcp: null, skillMarkdown: null, hook: structuredClone(definition.hook) });
    return { response, completion };
  }
  const response = pending("adopt_agent_catalog_asset");
  const completion = controller.adoptBinding("binding-claudeCode");
  await until(() => calls.some(call => call.command === "adopt_agent_catalog_asset"));
  return { response, completion };
}
function mountConsoleEditor(t: TestContext, context: ReturnType<typeof fresh>, controller: ReturnType<ConsoleModule["useAgentCatalogConsole"]>) {
  return mount(t, "AgentCatalogDefinitionModal", context.pinia, () => ({ visible: controller.editorVisible.value,
    definition: controller.editorDefinition.value, editingAssetId: controller.editorAssetId.value, category: controller.editorCategory.value,
    creatableCategories: ["hook"], loading: controller.editorLoading.value, saving: controller.editorSaving.value,
    error: controller.editorError.value, agents, preferredAgentKind: "gemini", onClose: controller.closeEditor, onSave: controller.saveDefinition }));
}
async function setEditorName(root: HostNode, name: string) {
  invoke(byAttribute(root, "placeholder", "输入便于识别的名称"), "onUpdate:modelValue", name); await nextTick();
}
function libraryRaceSnapshot(version = 2) {
  return hookSnapshot([hookAsset({ version, candidateIds: ["other-hook"] }),
    hookAsset({ id: "other-hook", name: "另一条 Hook", bindings: [hookBinding("claudeCode", "other-claude"), hookBinding("gemini", "other-gemini")] })]);
}

for (const mutation of ["save", "adopt"] as const) {
  test(`late ${mutation} in the same workspace preserves the newer draft and plan until both close`, async (t) => {
    const context = fresh(t, libraryRaceSnapshot()); const controller = context.console();
    const { response, completion } = await beginLibraryMutation(controller, mutation);
    controller.closeEditor(); controller.closeDetail(); await controller.openEditor(null, "hook");
    const editor = mountConsoleEditor(t, context, controller); await setEditorName(editor, "较新的未保存草稿");
    await prepare(controller, hookPlan("disable", "other-gemini", { assetId: "other-hook", token: "newer-plan" }));
    backendCatalog = libraryRaceSnapshot(3);
    response.resolve(hookDefinition({ version: 3 })); await completion; await settle();
    assert.equal(context.navigation.workspacePath, undefined);
    assert.equal(controller.editorVisible.value, true); assert.equal(controller.editorAssetId.value, null);
    assert.equal(byAttribute(editor, "placeholder", "输入便于识别的名称").props.modelValue, "较新的未保存草稿");
    assert.equal(controller.planVisible.value, true); assert.equal(controller.pendingPlan.value?.token, "newer-plan");
    assert.equal(controller.editorSaving.value, false); assert.equal(Object.values(controller.libraryBusy.value).some(Boolean), false);
    assert.equal(controller.selectedId.value, null); assert.equal(controller.editorError.value, "");
    assert.deepEqual(controller.libraryErrors.value, {});
    assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 0);

    controller.closeEditor(); await settle();
    assert.equal(controller.planVisible.value, true);
    assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 0);
    const refresh = pending("get_agent_asset_catalog"); controller.closePlan();
    await until(() => calls.some((call) => call.command === "get_agent_asset_catalog"));
    assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 1);
    refresh.resolve(backendCatalog); await settle();
    assert.equal(controller.catalog.value?.assets.find((asset) => asset.id === "global-hook")?.version, 3);
    assert.equal(calls.some((call) => call.command === "apply_agent_catalog"), false);
  });

  test(`obsolete ${mutation} failure cannot write errors into a newer same-workspace editor or plan`, async (t) => {
    const context = fresh(t, libraryRaceSnapshot()); const controller = context.console();
    const { response, completion } = await beginLibraryMutation(controller, mutation);
    controller.closeEditor(); controller.closeDetail(); await controller.openEditor(null, "hook");
    const editor = mountConsoleEditor(t, context, controller); await setEditorName(editor, "保留此草稿");
    await prepare(controller, hookPlan("disable", "other-gemini", { assetId: "other-hook", token: "current-plan" }));
    response.reject({ kind: "sourceConflict", message: "旧操作冲突" }); await completion; await settle();
    assert.equal(controller.editorVisible.value, true); assert.equal(controller.pendingPlan.value?.token, "current-plan");
    assert.equal(byAttribute(editor, "placeholder", "输入便于识别的名称").props.modelValue, "保留此草稿");
    assert.equal(controller.editorError.value, ""); assert.equal(controller.planError.value, "");
    assert.deepEqual(controller.libraryErrors.value, {});
    assert.equal(controller.editorSaving.value, false); assert.equal(Object.values(controller.libraryBusy.value).some(Boolean), false);
    assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 0);
  });
}

test("a deferred library read also preserves a draft and plan opened after that read begins", async (t) => {
  const context = fresh(t, libraryRaceSnapshot()); const controller = context.console();
  const { response, completion } = await beginLibraryMutation(controller, "save");
  controller.closeEditor(); await controller.openEditor(null, "hook");
  const editor = mountConsoleEditor(t, context, controller); await setEditorName(editor, "等待刷新期间的草稿");
  response.resolve(hookDefinition({ version: 3 })); await completion;
  const refresh = pending("get_agent_asset_catalog"); controller.closeEditor();
  await until(() => calls.some((call) => call.command === "get_agent_asset_catalog"));
  await controller.openEditor(null, "hook"); await nextTick(); await setEditorName(editor, "刷新开始后打开的新草稿");
  const preparation = prepare(controller, hookPlan("disable", "other-gemini", { assetId: "other-hook", token: "plan-after-read-start" }));
  refresh.resolve(libraryRaceSnapshot(3)); await preparation; await settle();
  assert.equal(controller.catalog.value?.assets.find((asset) => asset.id === "global-hook")?.version, 3);
  assert.equal(controller.editorVisible.value, true); assert.equal(controller.planVisible.value, true);
  assert.equal(byAttribute(editor, "placeholder", "输入便于识别的名称").props.modelValue, "刷新开始后打开的新草稿");
  assert.equal(controller.pendingPlan.value?.token, "plan-after-read-start");
  assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 1);
});

test("an explicit refresh preserves current drafts and consumes a deferred library refresh once", async (t) => {
  const context = fresh(t, libraryRaceSnapshot()); const controller = context.console();
  const { response, completion } = await beginLibraryMutation(controller, "save");
  controller.closeEditor(); await controller.openEditor(null, "hook");
  await prepare(controller, hookPlan("disable", "other-gemini", { assetId: "other-hook" }));
  response.resolve(hookDefinition({ version: 3 })); await completion;
  assert.equal(controller.editorVisible.value, true); assert.equal(controller.planVisible.value, true);
  const refresh = pending("get_agent_asset_catalog"); const refreshing = context.catalog.refresh();
  assert.equal(controller.editorVisible.value, true); assert.equal(controller.planVisible.value, true);
  await settle(); assert.equal(calls.filter((call) => call.command === "get_agent_asset_catalog").length, 1);
  refresh.resolve(libraryRaceSnapshot(3)); await refreshing;
  assert.equal(controller.catalog.value?.assets.find((asset) => asset.id === "global-hook")?.version, 3);
});
