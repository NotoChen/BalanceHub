import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test, { after, before, type TestContext } from "node:test";
import { compileScript, parse } from "@vue/compiler-sfc";
import { cloneVNode, createRenderer, defineComponent, h, nextTick, reactive, type Component } from "vue";
import { createServer, transformWithEsbuild, type ViteDevServer } from "vite";
import type AgentCatalogPanel from "../src/components/agent-workspace/AgentCatalogPanel.vue";
import type AgentCatalogRow from "../src/components/agent-workspace/AgentCatalogRow.vue";
import type { AgentAssetCatalog, AgentCatalogAsset, AgentCatalogAgentPanel, AgentCatalogUnresolvedTarget } from "../src/stores/agent-catalog-types.ts";
import type { AgentAssetCategory, AgentAssetProvenanceSummary, AgentAssetState, AgentCliKind } from "../src/stores/provider-types.ts";
import type { AgentWorkspacePage } from "../src/stores/agent-workspace.ts";
import { assetAction, assetSource } from "./agent-asset-fixtures.ts";
import { catalogAsset, catalogBinding, catalogObservations, catalogProvenanceSnapshot, catalogSnapshot, catalogTarget, workspaceAgent } from "./agent-workspace-fixtures.ts";

type PanelProps = InstanceType<typeof AgentCatalogPanel>["$props"];
type RowProps = InstanceType<typeof AgentCatalogRow>["$props"];
type HostNode = { type: string; props: Record<string, unknown>; style: Record<string, string>; text: string; children: HostNode[]; parent: HostNode | null };
const sourceRoot = fileURLToPath(new URL("../src/", import.meta.url));
const agents = [workspaceAgent("codex"), workspaceAgent("claudeCode"), workspaceAgent("gemini"), workspaceAgent("grok")];
const components = new Map<string, Component>();
let server: ViteDevServer;

before(async () => {
  Object.assign(globalThis, { document: { activeElement: null, body: {} }, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } });
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    configFile: false, server: { middlewareMode: true, hmr: false }, appType: "custom", logLevel: "silent",
    resolve: { alias: [{ find: /^@arco-design\/web-vue\/es\/icon$/, replacement: "virtual:catalog-row-icons" }] },
    plugins: [{
      name: "catalog-row-components",
      resolveId(id) { if (id === "virtual:catalog-row-icons") return "\0catalog-row-icons"; },
      async load(id) {
        if (id === "\0catalog-row-icons") return ["IconCommand", "IconApps", "IconBook", "IconBranch", "IconCode", "IconDashboard", "IconHistory", "IconLink", "IconThunderbolt"]
          .map((name) => `export const ${name} = { render() { return null; } };`).join("\n");
        if (!id.startsWith(sourceRoot) || !id.endsWith(".vue")) return;
        const { descriptor, errors } = parse(readFileSync(id, "utf8"), { filename: id });
        assert.deepEqual(errors, []);
        const compiled = compileScript(descriptor, { fs: { fileExists: existsSync, readFile: (path) => readFileSync(path, "utf8") }, id, inlineTemplate: true, templateOptions: { compilerOptions: { hoistStatic: false } } });
        return (await transformWithEsbuild(compiled.content, `${id}.ts`, { loader: "ts", target: "esnext" })).code;
      },
    }],
  });
  for (const name of ["AgentCatalogPanel", "AgentCatalogRow", "AgentCatalogFeatures"]) components.set(name,
    (await server.ssrLoadModule(`/src/components/agent-workspace/${name}.vue`)).default as Component);
});
after(async () => { await server?.close(); });

function node(type: string, text = ""): HostNode { return { type, props: {}, style: {}, text, children: [], parent: null, querySelectorAll: () => [] } as HostNode; }
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
function byAttribute(root: HostNode, name: string, value: unknown) {
  const matches = descendants(root).filter((element) => element.props[name] === value);
  assert.equal(matches.length, 1, `Expected one ${name}=${value}`); return matches[0];
}
function assertFeatureBadges(root: HostNode, labels: string[]) {
  const badges = descendants(root).filter((element) => element.props["data-asset-feature"]);
  assert.deepEqual(badges.map((badge) => badge.props["aria-label"]), labels);
  const glyphs = badges.map((badge) => {
    assert.equal(badge.type, "button");
    assert.equal(badge.props.type, "button");
    assert.equal(badge.parent?.props.content, badge.props["aria-label"]);
    assert.deepEqual(badge.parent?.props.trigger, ["hover", "focus"]);
    const svg = descendants(badge).find((element) => element.type === "svg");
    if (!svg) { assert.equal(textContent(badge), badge.props["aria-label"]); return null; }
    assert.equal(svg.props["aria-hidden"], "true");
    return svg.props.class;
  }).filter((glyph) => glyph !== null);
  assert.equal(new Set(glyphs).size, glyphs.length, "detail facts have distinguishable glyphs");
}
function invoke(element: HostNode, event = "onClick", value?: unknown) {
  if (element.props.disabled === true || element.props.disabled === "") return;
  const handler = element.props[event]; assert.equal(typeof handler, "function", `Expected ${event} on ${element.type}`);
  if (typeof handler === "function") handler(value);
}
function mount(t: TestContext, name: string, props: () => Record<string, unknown>) {
  const component = components.get(name); assert.ok(component);
  const app = renderer.createApp(defineComponent({ setup() { return () => h(component, props()); } }));
  for (const [name, tag] of [["a-button", "button"], ["a-select", "select"], ["a-option", "option"], ["a-tooltip", "span"]]) {
    app.component(name, defineComponent({ inheritAttrs: false, setup(_, { attrs, slots }) { return () => h(tag, attrs, [slots.icon?.(), slots.default?.()]); } }));
  }
  app.component("a-popover", defineComponent({ props: { popupVisible: Boolean }, emits: ["popupVisibleChange"], setup(props, { slots, emit }) {
    return () => h("span", { "data-popover": "true" }, [
      ...(slots.default?.() ?? []).map((child) => cloneVNode(child, { onClick: () => emit("popupVisibleChange", !props.popupVisible) })),
      props.popupVisible ? slots.content?.() : null,
    ]);
  } }));
  const root = node("root"); app.mount(root); t.after(() => app.unmount()); return root;
}
function mountRow(t: TestContext, asset: AgentCatalogAsset, overrides: Partial<RowProps> = {}) {
  const props = reactive<RowProps>({ asset,
    labels: new Map(agents.map((agent) => [agent.kind, agent.label])), busy: false, focused: false, ...overrides });
  const events: unknown[][] = [];
  const root = mount(t, "AgentCatalogRow", () => ({ ...props,
    onDetail: (id: string, feature?: string) => events.push(feature ? ["detail", id, feature] : ["detail", id]), onAction: (...args: unknown[]) => events.push(["action", ...args]),
    onManage: (id: string, kind: AgentCliKind | null) => { events.push(["manage", id, kind]); props.agentSelection = kind ? { assetId: id, agentKind: kind } : null; },
    onRetry: () => events.push(["retry"]), onNative: (id: string) => events.push(["native", id]),
  }));
  return { root, props, events, control: (kind: AgentCliKind) => byAttribute(root, "data-agent-kind", kind) };
}
function mountPanel(t: TestContext, catalog: AgentAssetCatalog, overrides: Partial<PanelProps> = {}) {
  const props = reactive<PanelProps>({ catalog, page: "mcp", query: "", agentFilter: null, agents,
    loading: false, error: "", busy: () => false, ...overrides });
  const events: unknown[][] = [];
  const root = mount(t, "AgentCatalogPanel", () => ({ ...props,
    onDetail: (id: string, feature?: string) => events.push(feature ? ["detail", id, feature] : ["detail", id]), onAction: (...args: unknown[]) => events.push(["action", ...args]),
    onManage: (id: string, kind: AgentCliKind | null) => { events.push(["manage", id, kind]); props.agentSelection = kind ? { assetId: id, agentKind: kind } : null; },
    onRetry: () => events.push(["retry"]), onNative: (id: string) => events.push(["native", id]),
  }));
  return { root, props, events };
}
function binding(kind: AgentCliKind, id: string, state: AgentAssetState) {
  const value = catalogBinding(kind, id);
  value.native.effectiveState = state;
  value.native.declaredState = state;
  value.actions = [assetAction("enable"), assetAction("disable")];
  return value;
}
function unresolved(fields: Partial<AgentCatalogUnresolvedTarget> = {}): AgentCatalogUnresolvedTarget {
  return { targetId: "saved-codex-target", contextId: "saved-codex-context", agentKind: "codex", scope: "user",
    drift: "inSync", appliedVersion: 1, state: "suspended", message: "定义已保留",
    actions: [assetAction("enable"), assetAction("remove")], ...fields };
}


function scopePanel(assetId: string, kind: AgentCliKind, fields: Partial<AgentCatalogAgentPanel> = {}): AgentCatalogAgentPanel {
  return { assetId, agentKind: kind, revision: "catalog-revision", observation: { agentKind: kind, state: "observed", reason: null }, entries: [
    { targetId: "exact-target", targetKind: "binding", contextId: "actual-context", scope: "user", label: "实际用户配置", path: "/fixture/actual/config",
      stateLabel: "已启用", reason: null, diagnostics: [], actions: [{ action: "disable", label: "停用", targetIds: ["exact-target"], available: true, reason: null, affectedAssetIds: [], parentNativeAssetId: null }] },
  ], ...fields };
}
function buttonWithText(root: HostNode, text: string) {
  const matches = descendants(root).filter((item) => item.type === "button" && textContent(item) === text);
  assert.equal(matches.length, 1, text); return matches[0];
}

test("resource rows show observed Agents and never dispatch a state-dependent action", async (t) => {
  const asset = catalogProvenanceSnapshot().assets[0];
  const row = mountRow(t, asset, { focused: true, agentPanel: scopePanel(asset.id, "codex") });
  const article = byAttribute(row.root, "data-global-asset-id", asset.id);
  assert.equal(textContent(byAttribute(article, "aria-label", `打开 ${asset.name} 详情`)), asset.name);
  assert.equal(article.props["aria-current"], "true");
  assert.equal(descendants(article).filter((element) => element.props["data-agent-kind"]).length, 3);
  for (const kind of ["codex", "claudeCode", "gemini"] satisfies AgentCliKind[]) {
    const control = row.control(kind);
    assert.equal(control.props["aria-haspopup"], "dialog");
    assert.equal(control.props["aria-pressed"], undefined);
    assert.match(String(control.props["aria-label"]), /管理 .* 在 .* 中的使用/);
    invoke(control); await nextTick();
    assert.deepEqual(row.events.at(-1), ["manage", asset.id, kind]);
    assert.equal(row.control(kind).props["aria-expanded"], true);
    invoke(byAttribute(row.root, "aria-label", "关闭 Agent 使用面板")); await nextTick();
    assert.equal(row.control(kind).props["aria-expanded"], false);
  }
  assert.equal(row.events.some((event) => event[0] === "action"), false);
});

test("primary facts omit unconfirmed author and collection labels", (t) => {
  const asset = catalogProvenanceSnapshot().assets[0]; asset.candidateIds = ["candidate"];
  const row = mountRow(t, asset);
  assertFeatureBadges(row.root, ["共享库 v1", "Agent 自带", "插件提供", "随 Agent 内置", "原生扩展包", "共享目录", "另有 1 份同名资源", "3 份配置存在差异"]);
  invoke(byAttribute(row.root, "data-asset-feature", "installation-sharedFiles"));
  assert.deepEqual(row.events, [["detail", asset.id, "installation-sharedFiles"]]);
  const detail = mount(t, "AgentCatalogFeatures", () => ({ asset, placement: "detail" }));
  assertFeatureBadges(detail, ["共享库 v1", "Agent 自带", "插件提供", "随 Agent 内置", "原生扩展包", "共享目录", "另有 1 份同名资源", "3 份配置存在差异"]);
});

test("both requested multi-fact examples keep distinct glyphs in their appropriate information layer", (t) => {
  const cases: { provenance: AgentAssetProvenanceSummary; primary: string[]; candidate?: boolean }[] = [
    { provenance: { provisions: ["pluginProvided"], installations: ["nativePackage"], providers: ["unknown"] },
      primary: ["插件提供", "原生扩展包", "另有 1 份同名资源"], candidate: true },
    { provenance: { provisions: ["independent"], installations: ["sharedFiles", "linked"], providers: ["unknown"] },
      primary: ["共享目录", "链接引用"] },
    { provenance: { provisions: ["unknown"], installations: ["unknown"], providers: ["agentVendor", "thirdParty", "userDeclared", "unknown"] },
      primary: [] },
    { provenance: { provisions: [], installations: [], providers: [] }, primary: [] },
  ];
  for (const [index, value] of cases.entries()) {
    const asset = catalogAsset("features-" + index, { ownership: "observed", version: null, provenance: value.provenance, candidateIds: value.candidate ? ["candidate"] : [] });
    assertFeatureBadges(mountRow(t, asset).root, value.primary);
    assertFeatureBadges(mount(t, "AgentCatalogFeatures", () => ({ asset, placement: "detail" })), value.primary);
  }
});

test("Agent usage exposes all backend scopes and only exact backend actions enter preview", async (t) => {
  const asset = catalogAsset("scopes", { bindings: [binding("codex", "unrelated-native-binding", "disabled")] });
  asset.bindings[0].actions = [assetAction("enable")];
  const data = scopePanel(asset.id, "codex");
  data.entries.push({ ...data.entries[0], targetId: "project-target", scope: "workspace", label: "实际项目配置", stateLabel: "已停用",
    actions: [{ action: "enable", label: "启用", targetIds: ["project-target"], available: true, reason: null, affectedAssetIds: [], parentNativeAssetId: null }] });
  const row = mountRow(t, asset, { agentPanel: data });
  invoke(row.control("codex")); await nextTick();
  assert.equal(descendants(row.root).filter((entry) => entry.props["data-agent-target"]).length, 2);
  invoke(buttonWithText(byAttribute(row.root, "data-agent-target", "exact-target"), "停用"));
  invoke(buttonWithText(byAttribute(row.root, "data-agent-target", "project-target"), "启用"));
  assert.deepEqual(row.events.slice(-2), [["action", asset.id, "disable", ["exact-target"]], ["action", asset.id, "enable", ["project-target"]]]);
  assert.equal(row.control("codex").props["data-agent-state"], "disabled", "opening a plan does not optimistically toggle native state");
  invoke(buttonWithText(byAttribute(row.root, "data-agent-target", "project-target"), "查看来源"));
  assert.deepEqual(row.events.at(-1), ["detail", asset.id, "binding:project-target"]);
});

test("unknown and unavailable scopes retain precise reasons and a backend-provided parent entry", async (t) => {
  const asset = catalogAsset("unknown", { bindings: [binding("claudeCode", "unknown-binding", "unknown")] });
  asset.application.observations = catalogObservations().map((item) => item.agentKind === "claudeCode" ? { ...item, state: "unknown", reason: "YAML frontmatter 无法读取" } : item);
  const data = scopePanel(asset.id, "claudeCode", { observation: { agentKind: "claudeCode", state: "unknown", reason: "YAML frontmatter 无法读取" } });
  data.entries[0].stateLabel = "状态未知";
  data.entries[0].actions = [{ action: "enable", label: "启用", targetIds: ["exact-target"], available: false, reason: "由所属插件管理", affectedAssetIds: [], parentNativeAssetId: "parent-plugin" }];
  const row = mountRow(t, asset, { agentPanel: data });
  assert.equal(row.control("claudeCode").props["data-agent-state"], "unknown");
  invoke(row.control("claudeCode")); await nextTick();
  assert.match(textContent(row.root), /YAML frontmatter 无法读取/);
  assert.equal(descendants(row.root).some(item => item.type === "button" && textContent(item) === "启用"), false);
  assert.match(textContent(row.root), /由所属插件管理/);
  invoke(buttonWithText(row.root, "管理所属插件"));
  assert.deepEqual(row.events.at(-1), ["native", "parent-plugin"]);
  assert.equal(row.events.some((event) => event[0] === "action"), false);
  assert.doesNotMatch(textContent(row.root), /配置到此处/);
});

test("an unknown header budget exposes backend diagnostics and next steps without changing state or action permission", async (t) => {
  const asset = catalogAsset("header-budget", { category: "skill", bindings: [binding("claudeCode", "native-unknown", "unknown")] });
  const data = scopePanel(asset.id, "claudeCode");
  data.entries[0].stateLabel = "状态未知";
  data.entries[0].reason = null;
  data.entries[0].diagnostics = [{ kind: "truncated", limit: "frontmatterLines", accepted: 64, observedAtLeast: 65 }];
  data.entries[0].actions = [{ action: "removeBinding", label: "移除配置", targetIds: ["exact-target"], available: true, reason: null, affectedAssetIds: [], parentNativeAssetId: null }];
  const row = mountRow(t, asset, { agentPanel: data });
  invoke(row.control("claudeCode")); await nextTick();
  assert.equal(row.control("claudeCode").props["data-agent-state"], "unknown");
  assert.match(textContent(row.root), /状态未知/);
  assert.match(textContent(row.root), /Skill 头部超过 64 行读取上限.*BalanceHub 无法完整判断状态.*源文件.*Agent.*核对加载状态/);
  assert.doesNotMatch(textContent(row.root), /JSON 内容无法解析|已启用|已加载/);
  const remove = buttonWithText(row.root, "移除配置"); assert.equal(remove.props.disabled, false);
  invoke(remove); assert.deepEqual(row.events.at(-1), ["action", asset.id, "removeBinding", ["exact-target"]]);
});

test("pending, failed and busy panels stay closable and read retries are explicit", async (t) => {
  const asset = catalogAsset("pending");
  const row = mountRow(t, asset, { agentLoading: true, busy: true });
  invoke(row.control("codex")); await nextTick();
  assert.match(textContent(row.root), /正在读取状态/);
  invoke(byAttribute(row.root, "aria-label", "关闭 Agent 使用面板")); await nextTick();
  assert.equal(descendants(row.root).some((item) => item.props.role === "dialog"), false);
  row.props.agentLoading = false; row.props.agentError = "读取超时";
  invoke(row.control("codex")); await nextTick();
  assert.match(textContent(row.root), /读取超时/);
  invoke(buttonWithText(row.root, "重新读取"));
  assert.deepEqual(row.events.at(-1), ["retry"]);
  invoke(byAttribute(row.root, "aria-label", "关闭 Agent 使用面板")); await nextTick();
  row.props.agentError = ""; row.props.agentPanel = scopePanel(asset.id, "codex");
  invoke(row.control("codex")); await nextTick();
  assert.equal(buttonWithText(row.root, "停用").props.disabled, true);
  assert.notEqual(buttonWithText(row.root, "打开详情").props.disabled, true);
});

test("Agent filtering narrows status columns while source facts retain the full identity", async (t) => {
  const snapshot = catalogProvenanceSnapshot(); const asset = snapshot.assets[0];
  asset.unresolvedTargets = [unresolved({ agentKind: "grok", state: "unknown" })];
  const panel = mountPanel(t, snapshot, { page: "skill", agentFilter: "codex" });
  assert.equal(descendants(panel.root).filter((item) => item.props["data-agent-kind"]).length, 1);
  assert.equal(descendants(panel.root).some((item) => item.props["data-agent-kind"] === "claudeCode"), false);
  assert.equal(descendants(panel.root).some((item) => item.props["data-agent-kind"] === "gemini"), false);
  assert.equal(descendants(panel.root).some((item) => item.props["data-agent-kind"] === "grok"), false);
  assertFeatureBadges(panel.root, ["共享库 v1", "Agent 自带", "插件提供", "随 Agent 内置", "原生扩展包", "共享目录", "3 份配置存在差异"]);
  const more = byAttribute(panel.root, "aria-controls", "agent-catalog-skill-more-filters");
  invoke(more); await nextTick();
  invoke(byAttribute(panel.root, "aria-label", "筛选配置路径"), "onUpdate:modelValue", "source-codex"); await nextTick();
  assert.equal(descendants(panel.root).some((item) => item.props["data-agent-kind"] === "claudeCode"), false);
  assertFeatureBadges(panel.root, ["共享库 v1", "Agent 自带", "插件提供", "随 Agent 内置", "原生扩展包", "共享目录", "3 份配置存在差异"]);
});

test("empty unapplied definitions expose no invented source facts and local errors keep focus support", (t) => {
  const row = mountRow(t, catalogAsset("unapplied", { bindings: [], provenance: { provisions: [], installations: [], providers: [] } }), { error: "提交结果待核对" });
  assertFeatureBadges(row.root, ["共享库 v1", "尚未配置到 Agent"]);
  const failure = byAttribute(row.root, "role", "alert");
  assert.equal(failure.props.tabindex, "0"); assert.equal(failure.parent?.props.content, "提交结果待核对");
  assert.deepEqual(failure.parent?.props.trigger, ["hover", "focus"]);
  invoke(byAttribute(row.root, "aria-label", "将 Shared tool 配置到 Agent"));
  assert.deepEqual(row.events, [["action", "unapplied", "applyDefinition"]]);
});

test("every catalog category reuses the same resource management entry", async (t) => {
  for (const [category, page] of [["skill", "skill"], ["mcp", "mcp"], ["plugin", "extension"], ["extension", "extension"], ["hook", "hook"]] satisfies [AgentAssetCategory, AgentWorkspacePage][]) {
    const asset = catalogAsset("asset-" + category, { category });
    const panel = mountPanel(t, catalogSnapshot({ assets: [asset] }), { page });
    invoke(byAttribute(panel.root, "data-agent-kind", "codex")); await nextTick();
    assert.deepEqual(panel.events, [["manage", asset.id, "codex"]]);
  }
});
