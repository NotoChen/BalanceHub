import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test, { after, before, type TestContext } from "node:test";
import { compileScript, parse } from "@vue/compiler-sfc";
import { cloneVNode, createRenderer, defineComponent, h, isVNode, nextTick, reactive, type Component } from "vue";
import { createServer, transformWithEsbuild, type ViteDevServer } from "vite";
import type AgentOverviewCard from "../src/components/agent-workspace/AgentOverviewCard.vue";
import type { AgentLifecycleVersionState } from "../src/stores/agent-lifecycle-types.ts";
import { assetInstallation } from "./agent-asset-fixtures.ts";
import { lifecycleTarget, workspaceAgent } from "./agent-workspace-fixtures.ts";

type CardProps = InstanceType<typeof AgentOverviewCard>["$props"];
type HostNode = {
  type: string;
  props: Record<string, unknown>;
  text: string;
  children: HostNode[];
  parent: HostNode | null;
};

const cardFile = fileURLToPath(new URL("../src/components/agent-workspace/AgentOverviewCard.vue", import.meta.url));
let server: ViteDevServer;
let Card: Component;

before(async () => {
  // Compile the real templates for the in-memory renderer so clicks exercise
  // their event bindings, without a browser, Tauri process or real config.
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    configFile: false,
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
    logLevel: "silent",
    resolve: { alias: [{ find: /^@arco-design\/web-vue\/es\/icon$/, replacement: "virtual:agent-card-icons" }] },
    plugins: [{
      name: "agent-card-component-tests",
      resolveId(id) { if (id === "virtual:agent-card-icons") return "\0agent-card-icons"; },
      async load(id) {
        if (id === "\0agent-card-icons") return "import { h } from 'vue'; " + ["IconCommand", "IconApps", "IconBook", "IconBranch", "IconCode", "IconDashboard", "IconHistory", "IconLink", "IconStorage", "IconThunderbolt", "IconDown", "IconPlayArrow", "IconRefresh"].map(name => `export const ${name} = { render() { return h('svg'); } };`).join("\n");
        if (!id.includes("/src/") || !id.endsWith(".vue")) return;
        const { descriptor, errors } = parse(readFileSync(id, "utf8"), { filename: id });
        assert.deepEqual(errors, []);
        const compiled = compileScript(descriptor, { fs: { fileExists: existsSync, readFile: (path) => readFileSync(path, "utf8") },
          id,
          inlineTemplate: true,
          templateOptions: { compilerOptions: { hoistStatic: false } },
        });
        return (await transformWithEsbuild(compiled.content, `${id}.ts`, { loader: "ts", target: "esnext" })).code;
      },
    }],
  });
  Card = (await server.ssrLoadModule(cardFile)).default as Component;
});

after(async () => { await server?.close(); });

function hostNode(type: string, text = ""): HostNode {
  return { type, props: {}, text, children: [], parent: null };
}
function remove(node: HostNode) {
  if (node.parent) node.parent.children = node.parent.children.filter((child) => child !== node);
  node.parent = null;
}
function insert(node: HostNode, parent: HostNode, anchor: HostNode | null = null) {
  remove(node);
  const index = anchor ? parent.children.indexOf(anchor) : -1;
  parent.children.splice(index < 0 ? parent.children.length : index, 0, node);
  node.parent = parent;
}
const renderer = createRenderer<HostNode, HostNode>({
  patchProp(node, key, _previous, value) { node.props[key] = value; },
  insert,
  remove,
  createElement: (type) => hostNode(type),
  createText: (text) => hostNode("#text", text),
  createComment: (text) => hostNode("#comment", text),
  setText(node, text) { node.text = text; },
  setElementText(node, text) {
    for (const child of node.children) child.parent = null;
    node.children = [];
    node.text = text;
  },
  parentNode: (node) => node.parent,
  nextSibling(node) {
    if (!node.parent) return null;
    return node.parent.children[node.parent.children.indexOf(node) + 1] ?? null;
  },
  insertStaticContent(content, parent, anchor) {
    const node = hostNode("#static", content.replace(/<[^>]*>/g, ""));
    insert(node, parent, anchor);
    return [node, node];
  },
});

function descendants(node: HostNode): HostNode[] {
  return [node, ...node.children.flatMap(descendants)];
}
function visibleText(node: HostNode): string {
  return node.type === "#comment" || node.props["aria-hidden"] === "true" ? "" : node.text + node.children.map(visibleText).join("");
}
function click(button: HostNode) {
  if (button.props.disabled === true || button.props.disabled === "") return;
  const handler = button.props.onClick;
  assert.ok(typeof handler === "function", "Expected a clickable control");
  handler({ type: "click" });
}
function mountCard(t: TestContext, overrides: Partial<CardProps> = {}) {
  const installation = assetInstallation("card-codex", { executablePath: "/fixture/bin/codex", installedVersion: "1.2.3" });
  const props = reactive<CardProps>({
    agent: workspaceAgent(),
    installations: [installation],
    lifecycleTargets: [lifecycleTarget({
      installation,
      channelLabel: "已验证安装来源",
      version: { state: "notChecked", source: "unknown", latestVersion: null, checkedAt: null, lastSuccessAt: null, nextCheckAt: null, stale: false, message: "尚未检查" },
    })],
    counts: { skill: 33, mcp: 4, extension: 2 },
    inventoryReady: true,
    hookCount: 3,
    historyCount: { agentKind: "codex", loadedCount: 42, total: 42 },
    historyLoading: false,
    runningCount: 2,
    canLaunch: true,
    loading: false,
    selectedPath: "/fixture/bin/codex",
    selectedVersion: "1.2.3",
    ...overrides,
  });
  const events: string[] = [];
  const app = renderer.createApp(defineComponent({
    setup() {
      return () => h(Card, {
        ...props,
        onAssets: (category: "skill" | "mcp" | "extension") => events.push(`assets:${category}`),
        onHooks: () => events.push("hooks"),
        onHistory: () => events.push("history"),
        onRuntime: () => events.push("runtime"),
        onInstallation: () => events.push("installation"),
        onInstallationGuide: () => events.push("installationGuide"),
        onVersions: () => events.push("installation"),
        onLaunch: () => events.push("launch"),
      });
    },
  }));
  app.component("a-button", defineComponent({
    inheritAttrs: false,
    setup(_, { attrs, slots }) { return () => h("button", { ...attrs, type: "button" }, [slots.icon?.(), slots.default?.()]); },
  }));
  app.component("a-tooltip", defineComponent({
    inheritAttrs: false,
    setup(_, { slots }) { return () => slots.default?.(); },
  }));
  app.component("a-popover", defineComponent({
    props: { popupVisible: Boolean }, emits: ["popupVisibleChange"],
    setup(props, { slots, emit }) { return () => h("div", [
      ...(slots.default?.() ?? []).map((child) => isVNode(child) ? cloneVNode(child, { onClick: () => emit("popupVisibleChange", !props.popupVisible) }) : child),
      ...(props.popupVisible ? slots.content?.() ?? [] : []),
    ]); },
  }));
  const root = hostNode("root");
  app.mount(root);
  t.after(() => app.unmount());
  function button(label: string | RegExp) {
    const matches = descendants(root).filter((node) => node.type === "button"
      && (typeof label === "string" ? node.props["aria-label"] === label : label.test(String(node.props["aria-label"] ?? ""))));
    assert.equal(matches.length, 1, `Expected one button with label ${label}; actual: ${descendants(root).filter(node => node.type === "button").map(node => node.props["aria-label"]).join(" | ")}`);
    return matches[0];
  }
  return {
    root, events, button,
    text: () => visibleText(root),
    async update(values: Partial<CardProps>) { Object.assign(props, values); await nextTick(); },
  };
}

test("available card shows its native icon and selected version with direct management actions", (t) => {
  const card = mountCard(t);
  const image = descendants(card.root).find((node) => node.type === "img");
  assert.ok(image);
  assert.equal(image.props.alt, "Codex CLI");
  assert.match(String(image.props.src), /codex/i);
  const version = card.button("Codex CLI 版本与路径，1.2.3");
  assert.equal(visibleText(version), "1.2.3");
  click(version);
  click(card.button("Codex CLI 版本与路径，1.2.3"));
  click(card.button("启动 Codex CLI"));
  assert.deepEqual(card.events, ["installation", "installation", "launch"]);
  assert.doesNotMatch(card.text(), /当前启动版本|已验证安装来源|已安装|个安装|暂无活动会话|查看版本与安装来源|\/fixture\//);
  assert.equal(descendants(card.root).filter((node) => node.type === "button" && String(node.props["aria-label"]).includes("版本")).length, 2);
});

test("six statistics retain real counts and routes beside the grouped launch actions", async (t) => {
  const card = mountCard(t, { counts: { skill: 333, mcp: 44, extension: 12 }, hookCount: 123 });
  const group = descendants(card.root).find((node) => node.props["aria-label"] === "Codex CLI 资源与会话");
  assert.ok(group);
  const buttons = descendants(group).filter((node) => node.type === "button");
  assert.equal(buttons.length, 6);
  assert.deepEqual(buttons.map(visibleText), ["Skill333", "MCP44", "插件12", "Hook123", "历史42", "活跃2"]);
  for (const button of buttons) click(button);
  const footer = descendants(card.root).find((node) => node.type === "footer");
  assert.ok(footer);
  const footerButtons = descendants(footer).filter((node) => node.type === "button");
  assert.deepEqual(footerButtons.map((button) => button.props["aria-label"]), [
    "刷新 Codex CLI", "启动 Codex CLI",
  ]);
  assert.match(String(buttons[5].props.class), /\bis-running\b/);
  for (const button of buttons) {
    const icon = descendants(button).find((node) => node.type === "svg");
    assert.ok(icon);
    assert.equal(icon.props["aria-hidden"], "true");
    assert.equal(icon.props.focusable, "false");
  }
  assert.deepEqual(card.events, ["assets:skill", "assets:mcp", "assets:extension", "hooks", "history", "runtime"]);
  await card.update({ runningCount: 0 });
  const session = card.button("查看 Codex CLI 的 活跃会话，共 0 个");
  assert.equal(visibleText(session), "活跃0");
  assert.doesNotMatch(String(session.props.class), /\bis-running\b/);
  click(session);
  assert.deepEqual(card.events, ["assets:skill", "assets:mcp", "assets:extension", "hooks", "history", "runtime", "runtime"]);
});

test("uninstalled cards keep discovered configuration counts and open installation management", (t) => {
  const card = mountCard(t, {
    installations: [], lifecycleTargets: [], canLaunch: false,
    selectedPath: null, selectedVersion: null, runningCount: 0,
  });
  assert.equal(visibleText(card.button("Codex CLI 安装状态，未安装")), "未安装");
  assert.equal(visibleText(card.button("查看 Codex CLI 的 Skill，共 33 项")), "Skill33");
  assert.equal(visibleText(card.button("查看 Codex CLI 的 Hook，已配置 3 条规则")), "Hook3");
  assert.equal(visibleText(card.button("查看 Codex CLI 的 活跃会话，共 0 个")), "活跃0");
  click(card.button("查看 Codex CLI 官方安装说明"));
  click(card.button("查看 Codex CLI 的 Hook，已配置 3 条规则"));
  assert.deepEqual(card.events, ["installationGuide", "hooks"]);
  assert.doesNotMatch(card.text(), /暂无活动会话/);
});

test("initial detection stays distinct from an uninstalled state and leaves management usable", async (t) => {
  const card = mountCard(t, {
    installations: [], lifecycleTargets: [], canLaunch: false, loading: true,
    selectedPath: null, selectedVersion: null, hookCount: null,
  });
  const version = descendants(card.root).find(node => node.props.role === "status" && visibleText(node) === "检测中")!;
  assert.ok(version);
  assert.equal(visibleText(version), "检测中");
  const launch = card.button("启动 Codex CLI");
  assert.equal(launch.props.disabled, true);
  click(launch);
  click(card.button("查看 Codex CLI 的 Hook，正在读取"));
  assert.deepEqual(card.events, ["hooks"]);
  assert.doesNotMatch(card.text(), /未安装/);
  await card.update({ loading: false });
  assert.ok(card.button("Codex CLI 安装状态，未安装"));
  assert.ok(card.button("查看 Codex CLI 官方安装说明"));
});

test("unknown counts and a failed first inventory never masquerade as empty assets or an uninstalled Agent", async (t) => {
  const card = mountCard(t, {
    installations: [], lifecycleTargets: [], canLaunch: false, loading: true, inventoryReady: false,
    selectedPath: null, selectedVersion: null, hookCount: null, counts: { skill: null, mcp: null, extension: null },
  });
  assert.equal(visibleText(card.button("查看 Codex CLI 的 Skill，正在读取")), "Skill—");
  await card.update({ loading: false });
  assert.ok(card.button("Codex CLI 安装状态，状态未读取"));
  assert.equal(visibleText(card.button("查看 Codex CLI 的 MCP，数量未确认")), "MCP—");
  assert.doesNotMatch(card.text(), /Skill0|MCP0|插件0|未安装/);
  await card.update({ inventoryReady: true, counts: { skill: 0, mcp: 0, extension: 0 }, hookCount: 0 });
  assert.equal(visibleText(card.button("查看 Codex CLI 的 Skill，共 0 项")), "Skill0");
  assert.ok(card.button("Codex CLI 安装状态，未安装"));
});

test("unavailable installation evidence keeps its distinction and compact versions retain full evidence in the title", async (t) => {
  const card = mountCard(t, { selectedVersion: "codex-cli 1.2.3" });
  assert.equal(visibleText(card.button("Codex CLI 版本与路径，1.2.3")), "1.2.3");
  await card.update({ installations: [assetInstallation("unavailable", { availability: "unavailable" })],
    selectedPath: null, selectedVersion: null, canLaunch: false, lifecycleTargets: [] });
  assert.ok(card.button("Codex CLI 安装状态，安装不可用"));
  assert.doesNotMatch(card.text(), /未安装/);
});

test("an available Agent with an unknown version still respects the supplied launch capability", async (t) => {
  const card = mountCard(t, { selectedVersion: null, lifecycleTargets: [], installations: [assetInstallation("unknown", { installedVersion: null })] });
  assert.equal(visibleText(card.button("Codex CLI 版本与路径，版本未读取")), "版本未读取");
  assert.doesNotMatch(card.text(), /未安装|检测中/);
  click(card.button("启动 Codex CLI"));
  assert.deepEqual(card.events, ["launch"]);
  await card.update({ canLaunch: false });
  const launch = card.button("启动 Codex CLI");
  assert.equal(launch.props.disabled, true);
  click(launch);
  assert.deepEqual(card.events, ["launch"]);
  click(card.button("Codex CLI 版本与路径，版本未读取"));
  assert.deepEqual(card.events, ["launch", "installation"]);
});

test("refreshing known facts keeps the selected version and parent-provided launch action", (t) => {
  const card = mountCard(t, { loading: true });
  assert.equal(visibleText(card.button("Codex CLI 版本与路径，1.2.3")), "1.2.3");
  assert.doesNotMatch(card.text(), /检测中|未安装/);
  assert.equal(card.button("启动 Codex CLI").props.disabled, false);
  click(card.button("启动 Codex CLI"));
  assert.deepEqual(card.events, ["launch"]);
});

test("Hook loading and unobserved counts remain distinct from an observed zero and stay clickable", async (t) => {
  const card = mountCard(t, { hookCount: null, loading: true });
  let hook = card.button("查看 Codex CLI 的 Hook，正在读取");
  assert.equal(visibleText(hook), "Hook—");
  assert.equal(hook.props["aria-busy"], true);
  click(hook);
  await card.update({ loading: false });
  hook = card.button("查看 Codex CLI 的 Hook，数量未确认");
  assert.equal(visibleText(hook), "Hook—");
  assert.equal(hook.props["aria-busy"], undefined);
  click(hook);
  await card.update({ hookCount: 0 });
  hook = card.button("查看 Codex CLI 的 Hook，已配置 0 条规则");
  assert.equal(visibleText(hook), "Hook0");
  click(hook);
  assert.deepEqual(card.events, ["hooks", "hooks", "hooks"]);
});

test("history statistics distinguish exact totals, failed metadata reads and unconfirmed zero", async (t) => {
  const card = mountCard(t, { historyCount: null });
  let history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(visibleText(history), "历史—");
  assert.match(String(history.props.title), /主目录本身及已记录目录，数量尚未读取/);
  await card.update({ historyCount: { agentKind: "codex", loadedCount: 0, total: null } });
  history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(visibleText(history), "历史—");
  assert.doesNotMatch(String(history.props.title), /没有历史|共 0 条/);
  await card.update({ historyCount: { agentKind: "codex", loadedCount: 17, total: null }, historyError: "原生会话身份不可读" });
  history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(visibleText(history), "历史读取失败");
  assert.match(String(history.props.title), /会话数量读取失败：原生会话身份不可读/);
  await card.update({ historyCount: { agentKind: "codex", loadedCount: 0, total: 0 }, historyError: "" });
  history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(visibleText(history), "历史0");
  assert.match(String(history.props.title), /共 0 条历史会话/);
  await card.update({ historyCount: { agentKind: "codex", loadedCount: 1234567, total: 1234567 } });
  history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(visibleText(history), "历史1234567");
  assert.match(String(history.props.title), /共 1234567 条历史会话/);
});

test("pending history statistics and a failed read leave navigation, environment and launch usable", async (t) => {
  const card = mountCard(t, { historyCount: null, historyLoading: true });
  let history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(history.props["aria-busy"], true);
  assert.notEqual(history.props.disabled, true);
  assert.match(String(history.props.title), /正在统计/);
  assert.equal(visibleText(card.button("Codex CLI 版本与路径，1.2.3")), "1.2.3");
  assert.equal(card.button("启动 Codex CLI").props.disabled, false);
  click(history);
  click(card.button("Codex CLI 版本与路径，1.2.3"));
  click(card.button("启动 Codex CLI"));
  await card.update({ historyLoading: false, historyCount: null });
  history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(history.props["aria-busy"], undefined);
  assert.equal(visibleText(history), "历史—");
  click(history);
  await card.update({ historyCount: { agentKind: "codex", loadedCount: 42, total: 42 }, historyLoading: true });
  history = card.button(/^查看 Codex CLI 的 历史会话，/);
  assert.equal(visibleText(history), "历史42");
  click(history);
  assert.deepEqual(card.events, ["history", "installation", "launch", "history", "history"]);
});

test("the current installation exposes each version state in a compact header hint", async (t) => {
  const card = mountCard(t);
  for (const state of ["notChecked", "unknown", "checkFailed", "upToDate", "updateAvailable"] satisfies AgentLifecycleVersionState[]) {
    await card.update({ lifecycleTargets: [lifecycleTarget({
      installation: assetInstallation("installation-a", { installedVersion: "1.2.3" }),
      channelLabel: "已验证安装来源",
      version: { state, source: "npmRegistry", latestVersion: "9.0.0", checkedAt: null, lastSuccessAt: null, nextCheckAt: null, stale: false, message: "fixture detail only" },
    })] });
    const version = card.button("Codex CLI 版本与路径，1.2.3");
    assert.equal(visibleText(version), "1.2.3");
    const labels = { notChecked: "未检查", unknown: "版本待确认", checkFailed: "检查失败", upToDate: "已是最新", updateAvailable: "有更新" };
    assert.equal(visibleText(card.button("查看 Codex CLI 当前使用安装的版本状态")), labels[state]);
    assert.doesNotMatch(card.text(), /已验证安装来源|fixture detail only|9\.0\.0/);
  }
});

test("long native version text remains available from the compact version entry", (t) => {
  const version = "codex-cli 1.2.3-preview.20260915+fixture-build-identity";
  const card = mountCard(t, { selectedVersion: version, lifecycleTargets: [] });
  const entry = card.button(`Codex CLI 版本与路径，${version.replace("codex-cli ", "")}`);
  assert.equal(visibleText(entry), "1.2.3-preview.20260915+fixture-build-identity");
  assert.match(String(entry.props.title), /版本与路径/);
  click(entry);
  assert.deepEqual(card.events, ["installation"]);
});
