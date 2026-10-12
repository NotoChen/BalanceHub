import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test, { after, before, type TestContext } from "node:test";
import { compileScript, parse } from "@vue/compiler-sfc";
import { createPinia, disposePinia, setActivePinia } from "pinia";
import { createRenderer, defineComponent, h, nextTick, ref, type Component } from "vue";
import { createServer, transformWithEsbuild, type ViteDevServer } from "vite";
import type { AgentSessionQueryApi } from "../src/api/agent-sessions.ts";
import type { AgentSessionDetail, AgentSessionRow } from "../src/stores/agent-session-types.ts";
import type { Provider } from "../src/stores/provider-types.ts";
import { sessionDetail, sessionPage, sessionRow, sessionScope } from "./agent-session-fixtures.ts";
import { workspaceAgent } from "./agent-workspace-fixtures.ts";

type HistoryModule = typeof import("../src/composables/useWorkspaceSessionHistory.ts");
type CliModule = typeof import("../src/stores/cli-runtime.ts");
type HostNode = { type: string; text: string; props: Record<string, unknown>; children: HostNode[]; parent: HostNode | null };
const pickerPath = fileURLToPath(new URL("../src/components/WorkspacePickerModal.vue", import.meta.url));
const listPath = fileURLToPath(new URL("../src/components/agent-workspace/AgentSessionList.vue", import.meta.url));
const componentPaths = new Set([
  pickerPath, listPath,
  ...["WorkspaceSessionHistoryPanel", "AgentCliIcon", "agent-workspace/AgentSessionRelation", "agent-workspace/SessionHighlight"]
    .map((name) => fileURLToPath(new URL(`../src/components/${name}.vue`, import.meta.url))),
]);
let server: ViteDevServer;
let Picker: Component;
let List: Component;
let histories: HistoryModule;
let cliStores: CliModule;
const previousHTMLElement = Object.getOwnPropertyDescriptor(globalThis, "HTMLElement");

before(async () => {
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: class {} });
  server = await createServer({
    configFile: false, optimizeDeps: { noDiscovery: true, include: [] },
    server: { middlewareMode: true, hmr: false }, appType: "custom", logLevel: "silent",
    resolve: { alias: [
      { find: /^@arco-design\/web-vue$/, replacement: "virtual:workspace-messages" },
      { find: /^@arco-design\/web-vue\/es\/icon$/, replacement: "virtual:workspace-icons" },
      { find: /^@tanstack\/vue-virtual$/, replacement: "virtual:workspace-viewport" },
    ] },
    plugins: [{
      name: "workspace-session-interactions",
      resolveId(id) { if (id.startsWith("virtual:workspace-")) return `\0${id.slice("virtual:".length)}`; },
      async load(id) {
        if (id === "\0workspace-messages") return "export const Message = { success() {}, error() {}, info() {}, warning() {} };";
        if (id === "\0workspace-icons") return `const icon = { render() { return null; } };
          export { icon as IconDelete, icon as IconFolder, icon as IconHome, icon as IconLaunch,
            icon as IconRefresh, icon as IconRight, icon as IconUp, icon as IconSearch };`;
        // The host renderer has no DOM. Keep real virtual range calculation with a fixed viewport.
        if (id === "\0workspace-viewport") return `
          import { computed, unref } from "vue";
          import { useVirtualizer as actual } from "/node_modules/@tanstack/vue-virtual/dist/esm/index.js";
          export function useVirtualizer(options) {
            return actual(computed(() => ({ ...unref(options), getScrollElement: () => null,
              initialRect: { width: 800, height: 600 } })));
          }`;
        if (!id.endsWith(".vue")) return;
        if (!componentPaths.has(id)) return "export default { render() { return null; } };";
        const { descriptor, errors } = parse(readFileSync(id, "utf8"), { filename: id });
        assert.deepEqual(errors, []);
        const compiled = compileScript(descriptor, {
          fs: { fileExists: existsSync, readFile: (path) => readFileSync(path, "utf8") }, id, inlineTemplate: true,
          templateOptions: { compilerOptions: { hoistStatic: false } },
        });
        return (await transformWithEsbuild(compiled.content, `${id}.ts`, { loader: "ts", target: "esnext" })).code;
      },
    }],
  });
  [histories, cliStores] = await Promise.all([
    server.ssrLoadModule("/src/composables/useWorkspaceSessionHistory.ts"),
    server.ssrLoadModule("/src/stores/cli-runtime.ts"),
  ]);
  Picker = (await server.ssrLoadModule(pickerPath)).default as Component;
  List = (await server.ssrLoadModule(listPath)).default as Component;
});
after(async () => {
  await server?.close();
  if (previousHTMLElement) Object.defineProperty(globalThis, "HTMLElement", previousHTMLElement);
  else Reflect.deleteProperty(globalThis, "HTMLElement");
});

test("temporary CLI selects and continues from the real history list without reading details", async (t) => {
  let detailReads = 0;
  const second = sessionRow("second");
  second.session.workdir = "/fixture/unregistered-project";
  const surface = await mountPicker(t, [sessionRow("first"), second], async () => {
    detailReads += 1;
    throw new Error("selection must not read details");
  });
  assert.equal(surface.history.workspaceSessionRoleFilter.value, "main");
  assert.equal(descendants(surface.root).some((item) => String(item.props.class ?? "").includes("workspace-history")), false);
  assert.match(text(surface.root), /终端/);
  assert.match(text(surface.root), /最近更新优先/);
  assert.equal(surface.continueButton().props.disabled, true);
  click(surface.selectionButton("first")); await settle();
  assert.equal(surface.history.workspaceSelectedResumeId.value, "first");
  assert.equal(surface.history.workspaceSelectedSessionRef.value, "ref:first");
  assert.equal(surface.selectionButton("first").props["aria-pressed"], true);
  assert.equal(surface.history.workspaceSessionDetailVisible.value, false);
  assert.equal(detailReads, 0);
  click(surface.selectionButton("second")); await settle();
  assert.equal(surface.selectionButton("first").props["aria-pressed"], false);
  assert.equal(surface.selectionButton("second").props["aria-pressed"], true);
  click(surface.continueButton());
  assert.deepEqual(surface.launches, [{ path: "/fixture/unregistered-project", sessionRef: "ref:second" }]);
  assert.ok(text(surface.root).includes("/fixture/unregistered-project"));
  assert.equal(detailReads, 0);
});

test("optional details can close while loading and late results never replace the launch selection", async (t) => {
  const pending = deferred<AgentSessionDetail>();
  const surface = await mountPicker(t, [sessionRow("first"), sessionRow("second")], async () => pending.promise);
  click(surface.selectionButton("first")); await settle();
  click(surface.detailButton("second")); await settle();
  assert.equal(surface.history.workspaceSessionDetailVisible.value, true);
  assert.equal(surface.history.workspaceSessionDetailLoading.value, true);
  assert.equal(surface.history.workspaceSessionsLoading.value, false);
  assert.equal(surface.continueButton().props.disabled, false);
  surface.history.closeWorkspaceSessionDetail(); await settle();
  assert.equal(surface.history.workspaceSessionDetailVisible.value, false);
  assert.equal(surface.history.workspaceSessionDetailLoading.value, false);
  assert.equal(surface.history.workspaceSelectedSessionRef.value, "ref:first");
  pending.resolve(sessionDetail(sessionRow("second"))); await settle();
  assert.equal(surface.history.workspaceSessionDetailVisible.value, false);
  assert.equal(surface.history.workspaceSelectedSessionRef.value, "ref:first");
  click(surface.continueButton());
  assert.deepEqual(surface.launches, [{ path: "/fixture/home", sessionRef: "ref:first" }]);
});

test("read-only sessions remain viewable and detail failures do not block selecting another session", async (t) => {
  const readOnly = sessionRow("readonly", { resumeReason: "该子会话只读" });
  readOnly.session.canResume = false;
  const surface = await mountPicker(t, [readOnly, sessionRow("available")], async () => {
    throw new Error("fixture detail unavailable");
  });
  assert.equal(surface.selectionButton("readonly").props.disabled, true);
  assert.equal(surface.detailButton("readonly").props.disabled, false);
  assert.match(text(surface.root), /该子会话只读/);
  click(surface.detailButton("readonly")); await settle();
  assert.match(surface.history.workspaceSessionDetailError.value, /fixture detail unavailable/);
  assert.equal(surface.history.workspaceSessionDetailLoading.value, false);
  assert.equal(surface.history.workspaceSelectedSessionRef.value, "");
  surface.history.closeWorkspaceSessionDetail(); await settle();
  click(surface.selectionButton("available")); await settle();
  assert.equal(surface.continueButton().props.disabled, false);
  assert.equal(surface.history.workspaceSelectedSessionRef.value, "ref:available");
});

test("the session workbench keeps its primary row action for reading", async (t) => {
  const actions: string[] = [];
  const app = renderer.createApp(defineComponent({ setup: () => () => h(List, {
    rows: [sessionRow("browse")], onDetail: () => actions.push("detail"), onSelect: () => actions.push("select"),
  }) }));
  const root = node("root"); app.mount(root); t.after(() => app.unmount());
  const primary = descendants(root).find((item) => item.type === "button" && item.props.class === "agent-session-select");
  assert.ok(primary); click(primary);
  assert.deepEqual(actions, ["detail"]);
  assert.equal(primary.props["aria-pressed"], undefined);
  assert.ok(descendants(root).some((item) => item.type === "button" && text(item).includes("查看详情")));
});

async function mountPicker(t: TestContext, rows: AgentSessionRow[], detail: AgentSessionQueryApi["detail"]) {
  const pinia = createPinia(); setActivePinia(pinia);
  const cli = cliStores.useCliRuntimeStore();
  cli.cliEnvironmentProbe = { tools: [{ ...workspaceAgent(), available: true, path: "/fixture/bin/codex", version: "1.0.0", message: "" }] };
  cli.cliRuntime = { agents: [workspaceAgent()], configs: [] };
  const visible = ref(true);
  const directory = ref({ currentPath: "/fixture/workspace", homePath: "/fixture", parentPath: "/fixture", entries: [] });
  const provider = { displayLabel: "合成中转站", identity: { id: "fixture-provider" }, auth: { apiKey: "fixture-secret", apiKeyOptions: [] }, cli: { preferredModel: "" }, capabilities: { availableModels: [] } } as unknown as Provider;
  const launches: { path: string; sessionRef: string }[] = [];
  let history!: ReturnType<HistoryModule["useWorkspaceSessionHistory"]>;
  const app = renderer.createApp(defineComponent({ setup() {
    history = histories.useWorkspaceSessionHistory({
      visible, cliKind: ref("codex"), sessionMode: ref("history"), selectedModel: ref(""),
      sessionApi: { scope: async (path) => sessionScope(path),
        query: async (request) => sessionPage(rows, { scopeRevision: request.scopeRevision }), detail,
        message: async () => { throw new Error("unexpected message request"); }, cancel: async () => undefined },
    });
    return () => h(Picker, {
      visible: visible.value, provider, cliKind: "codex", cliOptions: [{ value: "codex", label: "Codex" }],
      apiKeys: [], apiKeyLoading: false, apiKeyError: "", apiKeyLocalId: "", selectedModel: "", sessionName: "", canNameSession: false,
      sessionMode: "history", selectedResumeId: history.workspaceSelectedResumeId.value,
      terminalKind: "terminal", terminalOptions: [{ value: "terminal", label: "系统终端" }], workspaces: [],
      directory: directory.value, pathDraft: directory.value.currentPath, browsing: false,
      launchPreviewVisible: false, launchPreviewLoading: false, forgettingPath: null, error: "",
      historyQuery: history.workspaceSessionQuery.value, historyResults: history.workspaceSessionResults.value,
      historyRoleFilter: history.workspaceSessionRoleFilter.value, historyTotal: history.workspaceSessionTotal.value,
      historyHasMore: history.workspaceSessionHasMore.value, historyLoadingMore: history.workspaceSessionsLoadingMore.value,
      selectedSessionRef: history.workspaceSelectedSessionRef.value, selectedSessionTitle: history.workspaceSelectedSessionTitle.value,
      selectedSessionWorkdir: history.workspaceSelectedSessionWorkdir.value,
      historyLoading: history.workspaceSessionsLoading.value, historyError: history.workspaceSessionsError.value,
      historyIndexState: history.workspaceSessionIndexState.value, historyIndexMessage: history.workspaceSessionIndexMessage.value,
      onSelectSession: history.selectWorkspaceSession, onViewSession: history.openWorkspaceSessionDetail,
      onLaunch: (path: string) => launches.push({ path, sessionRef: history.workspaceSelectedSessionRef.value }),
    });
  } }));
  app.use(pinia);
  app.component("a-modal", defineComponent({ props: { visible: Boolean }, setup(props, { slots }) { return () => props.visible ? h("section", [slots.title?.(), slots.default?.()]) : null; } }));
  for (const [name, tag] of [["a-button", "button"], ["a-select", "select"], ["a-option", "option"], ["a-input", "input"], ["a-tooltip", "span"], ["a-radio-group", "div"], ["a-radio", "label"], ["a-alert", "aside"], ["a-checkbox", "label"], ["a-spin", "div"]]) {
    app.component(name, defineComponent({ inheritAttrs: false, setup(_, { attrs, slots }) { return () => h(tag, attrs, [slots.icon?.(), slots.default?.()]); } }));
  }
  const root = node("root"); app.mount(root);
  t.after(async () => { app.unmount(); disposePinia(pinia); await settle(); });
  await history.loadWorkspaceSessions(); await settle();
  function rowButton(id: string, prefix: string) {
    const row = descendants(root).find((item) => item.props["data-session-ref"] === `ref:${id}`);
    assert.ok(row);
    const button = descendants(row).find((item) => item.type === "button" && String(item.props["aria-label"] ?? "").startsWith(prefix));
    assert.ok(button); return button;
  }
  function continueButton() {
    const button = descendants(root).find((item) => item.type === "button" && text(item).replace(/\s+/g, " ").trim() === `继续 ${workspaceAgent().label}`);
    assert.ok(button); return button;
  }
  return { root, history, launches, continueButton, selectionButton: (id: string) => rowButton(id, "选择会话："), detailButton: (id: string) => rowButton(id, "查看会话详情：") };
}

function node(type: string, text = ""): HostNode { return { type, text, props: {}, children: [], parent: null }; }
function remove(child: HostNode) { if (child.parent) child.parent.children = child.parent.children.filter((item) => item !== child); child.parent = null; }
function insert(child: HostNode, parent: HostNode, anchor: HostNode | null = null) {
  remove(child); const position = anchor ? parent.children.indexOf(anchor) : -1;
  parent.children.splice(position < 0 ? parent.children.length : position, 0, child); child.parent = parent;
}
const renderer = createRenderer<HostNode, HostNode>({
  patchProp(element, key, _previous, value) { element.props[key] = value; }, insert, remove,
  createElement: (type) => node(type), createText: (text) => node("#text", text), createComment: (text) => node("#comment", text),
  setText(element, text) { element.text = text; }, setElementText(element, text) { element.text = text; element.children = []; },
  parentNode: (element) => element.parent,
  nextSibling(element) { return element.parent?.children[element.parent.children.indexOf(element) + 1] ?? null; },
  insertStaticContent(content, parent, anchor) { const child = node("#static", content.replace(/<[^>]*>/g, "")); insert(child, parent, anchor); return [child, child]; },
});
function descendants(root: HostNode): HostNode[] { return [root, ...root.children.flatMap(descendants)]; }
function text(root: HostNode): string { return root.type === "#comment" ? "" : root.text + root.children.map(text).join(""); }
function click(button: HostNode) {
  assert.ok(button.props.disabled !== true && button.props.disabled !== "", "action must be enabled");
  assert.equal(typeof button.props.onClick, "function");
  (button.props.onClick as () => void)();
}
async function settle() { for (let index = 0; index < 24; index += 1) { await Promise.resolve(); await nextTick(); } }
function deferred<T>() { let resolve!: (value: T) => void; return { promise: new Promise<T>((done) => { resolve = done; }), resolve: (value: T) => resolve(value) }; }
