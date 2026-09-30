import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test, { after, before } from "node:test";
import { fileURLToPath } from "node:url";
import { createPinia, setActivePinia } from "pinia";
import { createRenderer, createSSRApp, defineComponent, h } from "vue";
import { renderToString } from "@vue/server-renderer";
import { createServer, type ViteDevServer } from "vite";
import type { AppSettings, CliEnvironmentProbeResult, AgentEnvironmentInventory, AgentInstallation, AgentAssetReadResult, AgentAssetSource } from "../src/stores/provider-types.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (path: string) => readFileSync(join(root, path), "utf8");
type StoreModule = typeof import("../src/stores/agent-environment.ts");
type HookStoreModule = typeof import("../src/stores/agent-hooks.ts");
type CenterModule = typeof import("../src/composables/useAgentEnvironmentCenter.ts");
type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
};

let server: ViteDevServer;
let storeModule: StoreModule;
let hookStoreModule: HookStoreModule;
const commandQueues = new Map<string, Deferred<unknown>[]>();

before(async () => {
  Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      invoke(command: string) {
        const pending = commandQueues.get(command)?.shift();
        if (!pending) throw new Error(`Unexpected Tauri command: ${command}`);
        return pending.promise;
      },
    },
  });
  server = await createServer({ optimizeDeps: { noDiscovery: true, include: [] }, server: { middlewareMode: true }, appType: "custom", logLevel: "silent" });
  storeModule = await server.ssrLoadModule("/src/stores/agent-environment.ts") as StoreModule;
  hookStoreModule = await server.ssrLoadModule("/src/stores/agent-hooks.ts") as HookStoreModule;
});

after(async () => {
  await server.close();
  commandQueues.clear();
});

test("Agent environment API keeps the Rust command boundary explicit", () => {
  const source = read("src/api/app.ts");
  assert.match(source, /get_agent_environment_inventory/);
  assert.match(source, /read_agent_environment_asset/);
  assert.match(source, /open_agent_environment_asset/);
  assert.match(source, /read_agent_environment_source/);
  assert.match(source, /open_agent_environment_source/);
  assert.doesNotMatch(source, /openPath|open_path/);
});

test("Agent environment TypeScript mirrors the strict Rust declaration and relationship shape", () => {
  const types = read("src/stores/provider-types.ts");
  const nativeRef = types.match(
    /export interface AgentAssetNativeRef \{([\s\S]*?)\n\}/,
  )?.[1];
  assert.ok(nativeRef);
  assert.match(nativeRef, /category: AgentAssetCategory;/);
  assert.match(nativeRef, /nativeId: string;/);
  assert.match(nativeRef, /qualifier: string \| null;/);
  assert.doesNotMatch(nativeRef, /declarationKey/);

  const declaration = types.match(
    /export interface AgentAssetDeclaration \{([\s\S]*?)\n\}/,
  )?.[1];
  assert.ok(declaration);
  assert.match(declaration, /role: AgentAssetDeclarationRole;/);
  assert.match(declaration, /participation: AgentAssetResolutionParticipation;/);
  assert.match(declaration, /providedBy: AgentAssetNativeRef \| null;/);
  assert.match(declaration, /actionOwner: AgentAssetNativeRef \| null;/);
  assert.match(declaration, /explicitlyAffected: AgentAssetNativeRef\[\];/);
});

test("Skill diagnostics explain YAML errors, bounded reads and unobserved native state separately", async () => {
  const { formatAgentAssetDiagnostic: format } = await server.ssrLoadModule(
    "/src/utils/agent-environment-diagnostics.ts",
  ) as typeof import("../src/utils/agent-environment-diagnostics.ts");
  for (const [location, expected] of [
    ["frontmatter.encoding", /UTF-8.*编码.*刷新/],
    ["frontmatter.unterminated", /缺少结束分隔线.*SKILL\.md.*刷新/],
    ["frontmatter.syntax", /YAML.*语法.*字段重复.*刷新/],
    ["frontmatter.root", /YAML.*字段映射.*刷新/],
    ["frontmatter.name", /name.*非空单行文本.*刷新/],
    ["frontmatter.disable-model-invocation", /disable-model-invocation.*true.*false.*刷新/],
    ["frontmatter.complexity", /复杂度上限.*BalanceHub 无法完整判断状态.*源文件.*Agent/],
  ] as const) {
    const message = format({ kind: "malformed", format: "yaml", location });
    assert.match(message, expected);
    assert.doesNotMatch(message, /JSON|不支持|已启用|已加载/);
  }
  for (const [limit, accepted, unit] of [
    ["frontmatterLines", 64, "行"],
    ["frontmatterBytes", 16384, "字节"],
  ] as const) {
    const message = format({ kind: "truncated", limit, accepted, observedAtLeast: accepted + 1 });
    assert.match(message, new RegExp(`${accepted} ${unit}读取上限`));
    assert.match(message, /BalanceHub 无法完整判断状态.*源文件.*Agent/);
    assert.doesNotMatch(message, /语法有误|已启用|已加载/);
  }
  assert.match(format({ kind: "readFailed", sourceId: "source:fixture", errorKind: "permissionDenied" }), /权限不足.*检查文件权限.*刷新/);
  assert.match(format({ kind: "discoveryIncomplete", agentKind: "claudeCode", category: "skill", reason: "runtimeStateUnobserved" }), /已读取静态配置.*Agent 运行时确定/);
  assert.match(format({ kind: "discoveryIncomplete", agentKind: "claudeCode", category: "skill", reason: "unsupportedVersion" }), /BalanceHub 尚未适配/);
  assert.equal(format({ kind: "malformed", format: "yaml", location: null }), "YAML 内容无法解析");
  assert.equal(format({ kind: "malformed", format: "json", location: "statusLine" }), "JSON 内容无法解析（statusLine）");
});

test("managed Hook API exposes plans and owned apply without accepting paths", () => {
  const source = read("src/api/app.ts");
  assert.match(source, /inspect_agent_hook/);
  assert.match(source, /plan_agent_hook/);
  assert.match(source, /apply_agent_hook/);
  assert.match(source, /health_agent_hook/);
  assert.match(source, /repair_agent_hook/);
  assert.match(source, /verify_agent_hook/);
  assert.doesNotMatch(source, /applyAgentHook\([^)]*path/);
});

test("managed Hook store rejects stale inspections and releases failed apply state", async () => {
  setActivePinia(createPinia());
  commandQueues.clear();
  const store = hookStoreModule.useAgentHookStore();
  const oldRequest = queueCommand("inspect_agent_hook");
  const newRequest = queueCommand("health_agent_hook");
  const oldPromise = store.inspect("codex");
  const newPromise = store.inspect("codex", "health");

  newRequest.resolve(hookInspectionFixture("healthy", "new"));
  await newPromise;
  oldRequest.resolve(hookInspectionFixture("not_installed", "old"));
  await oldPromise;

  assert.equal(store.inspections["codex:native"]?.state, "healthy");
  assert.equal(store.inspections["codex:native"]?.revision, "new");
  assert.equal(store.states["codex:native"], "ready");

  const failedApply = queueCommand("apply_agent_hook");
  const applyPromise = store.apply("codex", hookPlanFixture());
  failedApply.reject(new Error("revision changed"));
  await assert.rejects(applyPromise, /revision changed/);
  assert.equal(store.states["codex:native"], "error");
  assert.equal(store.errors["codex:native"], "revision changed");
});

test("managed Hook store rejects cross-target responses and unsupported WSL management", async () => {
  setActivePinia(createPinia());
  commandQueues.clear();
  const store = hookStoreModule.useAgentHookStore();
  const mismatched = queueCommand("inspect_agent_hook");
  const pending = store.inspect("codex");
  mismatched.resolve({ ...hookInspectionFixture("healthy", "wrong"), agentKind: "claude" });
  await assert.rejects(pending, /请求目标不一致/);
  assert.equal(store.inspections["codex:native"], undefined);
  assert.equal(store.states["codex:native"], "error");

  await assert.rejects(
    store.inspect("codex", "inspect", { kind: "wsl", distro_id: "Ubuntu" }),
    /暂不支持管理 WSL/,
  );
});

test("managed Hook plan modal stays closable and applies only after explicit confirmation", () => {
  const source = read("src/components/settings/agent-environment/AgentHookPlanModal.vue");
  assert.match(source, /closable/);
  assert.match(source, /mask-closable/);
  assert.match(source, /esc-to-close/);
  assert.match(source, /确认应用/);
  assert.match(source, /emit\('confirm'\)/);
  assert.doesNotMatch(source, /installationAvailable/);
  assert.doesNotMatch(source, /balancehub-critical-modal-lock/);
});

test("Agent environment remains a per-tool detail while primary management moves out of settings", () => {
  const dashboard = read("src/components/agent-workspace/AgentDashboard.vue");
  const detail = read("src/components/agent-workspace/AgentInstallationModal.vue");
  const row = read("src/components/settings/agent-environment/AgentEnvironmentRow.vue");
  const settings = read("src/components/settings/SettingsTerminalSection.vue");
  assert.match(dashboard, /AgentOverviewCard/);
  assert.match(dashboard, /AgentCatalogPanel/);
  assert.match(detail, /AgentInstallationTarget/);
  assert.doesNotMatch(settings, /SettingsAgentEnvironmentCenter|AgentEnvironmentConsole/);
  assert.match(row, /<article class="agent-environment-row"/);
  assert.match(row, /emit\('detail'/);
  assert.match(row, /healthAction\?\.available/);
  assert.match(row, /verifyAction\?\.available/);
});

test("Agent detail keeps complete read-only Hook diagnostics without mutation controls", () => {
  const detail = read("src/components/settings/agent-environment/AgentInstallationDetail.vue");
  assert.match(detail, /会话 Hook 诊断/);
  assert.match(detail, /hookInspection\.diagnostics/);
  assert.match(detail, /hookInspection\.ownership\.resources/);
  assert.match(detail, /hookInspection\.configPath/);
  assert.doesNotMatch(detail, /applyAgentHook|planAgentHook|emit\('mutate'/);
});

test("Rust owns Hook action availability in the IPC contract", () => {
  const model = read("src-tauri/src/models/agent_hook.rs");
  const types = read("src/stores/provider-types.ts");
  assert.match(model, /AgentHookActionKind/);
  assert.match(model, /pub actions: Vec<AgentHookAction>/);
  assert.match(types, /actions: AgentHookAction\[\]/);
  assert.match(types, /reason: string \| null/);
});

test("Agent environment store rejects stale inventory writes and preserves the last success", async () => {
  const store = freshStore();
  const oldRequest = queueCommand("get_agent_environment_inventory");
  const newRequest = queueCommand("get_agent_environment_inventory");
  const oldPromise = store.loadInventory(undefined, true);
  const newPromise = store.loadInventory(undefined, true);

  newRequest.resolve(inventoryFixture("new"));
  await newPromise;
  oldRequest.resolve(inventoryFixture("old"));
  await oldPromise;

  assert.equal(store.inventory()?.scannedAt, "new");
  assert.equal(store.inventoryState.__native__, "ready");

  const failedRefresh = queueCommand("get_agent_environment_inventory");
  const failedPromise = store.loadInventory(undefined, true);
  failedRefresh.reject(new Error("offline"));
  await assert.rejects(failedPromise, /offline/);
  assert.equal(store.inventory()?.scannedAt, "new");
  assert.equal(store.inventoryState.__native__, "error");
  assert.equal(store.inventoryErrors.__native__, "offline");
});

test("a superseded inventory failure cannot replace a newer ready state", async () => {
  const store = freshStore();
  const oldRequest = queueCommand("get_agent_environment_inventory");
  const newRequest = queueCommand("get_agent_environment_inventory");
  const oldPromise = store.loadInventory(undefined, true);
  const newPromise = store.loadInventory(undefined, true);

  newRequest.resolve(inventoryFixture("current"));
  await newPromise;
  oldRequest.reject(new Error("stale failure"));
  await assert.rejects(oldPromise, /stale failure/);

  assert.equal(store.inventory()?.scannedAt, "current");
  assert.equal(store.inventoryState.__native__, "ready");
  assert.equal(store.inventoryErrors.__native__, null);
});

test("a late config preview cannot replace the newer preview for the same source", async () => {
  const store = freshStore();
  const oldRequest = queueCommand("read_agent_environment_source");
  const newRequest = queueCommand("read_agent_environment_source");
  const access = { accessId: "access:config", environmentId: "native:macos" };
  const oldPromise = store.readConfigPreview("source:config", access);
  const newPromise = store.readConfigPreview("source:config", access);

  newRequest.resolve(previewFixture("new content"));
  await newPromise;
  oldRequest.resolve(previewFixture("old content"));
  await oldPromise;

  assert.equal(Object.values(store.previews)[0]?.content, "new content");
  assert.deepEqual(Object.values(store.previewState), ["ready"]);
});

test("closing or unmounting ignores late local results without invalidating shared cache", async () => {
  const centerModule = await server.ssrLoadModule("/src/composables/useAgentEnvironmentCenter.ts");
  let center: ReturnType<typeof centerModule.useAgentEnvironmentCenter>;
  const pinia = createPinia();
  setActivePinia(pinia);
  const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: (type) => ({ type }),
    createText: (text) => ({ text }),
    createComment: (text) => ({ text }),
    setText(node, text) { node.text = text; },
    setElementText(node, text) { node.text = text; },
    parentNode: () => null,
    nextSibling: () => null,
    querySelector: () => null,
    setScopeId() {},
    cloneNode: (node) => ({ ...node }),
    insertStaticContent: () => [{}, {}],
  });
  const app = renderer.createApp(defineComponent({
    setup() {
      center = centerModule.useAgentEnvironmentCenter();
      return () => h("div");
    },
  }));
  app.use(pinia);
  app.mount({});
  const store = storeModule.useAgentEnvironmentStore();
  const fixture = inventoryFixture("before preview");
  fixture.sources = [sourceFixture()];
  store.inventories.__native__ = fixture;
  const previewRequest = queueCommand("read_agent_environment_source");
  const previewPromise = center!.assets.action("source", "source:config", "preview");

  center!.assets.closeDetail();
  previewRequest.resolve(previewFixture("late preview"));
  await previewPromise;
  assert.equal(center!.assets.preview.value, null);
  assert.equal(center!.assets.previewState.value, "idle");
  assert.equal(Object.values(store.previews)[0]?.content, "late preview");
  assert.deepEqual(Object.values(store.previewState), ["ready"]);

  const inventoryRequest = queueCommand("get_agent_environment_inventory");
  const inventoryPromise = store.loadInventory(undefined, true);
  app.unmount();
  inventoryRequest.resolve(inventoryFixture("late inventory"));
  await inventoryPromise;
  assert.equal(store.inventories.__native__?.scannedAt, "late inventory");
  assert.equal(store.inventoryState.__native__, "ready");
});

test("Agent environment configuration preview is bounded and read-only", () => {
  const source = read("src/components/settings/agent-environment/AgentAssetPreview.vue");
  const styles = read("src/styles/modules/agent-environment.css");
  assert.match(source, /内容已截断/);
  assert.match(source, /仅元数据/);
  assert.match(source, /没有可预览的内容/);
  assert.match(read("src/styles/modules/code-editor.css"), /\.code-editor \.cm-scroller[^}]*overflow: auto/);
  assert.match(styles, /\.agent-config-preview[\s\S]*min-height: 286px/);
});

test("metadata-only previews and unknown versions render without inventing failure or update state", async () => {
  const configModule = await server.ssrLoadModule(
    "/src/components/settings/agent-environment/AgentAssetPreview.vue",
  );
  const versionModule = await server.ssrLoadModule(
    "/src/components/agent-workspace/AgentVersionStatus.vue",
  );
  const configApp = createSSRApp(configModule.default, {
    preview: {
      ...previewFixture(""),
      content: null,
      metadataOnly: true,
      diagnostics: ["sensitiveFileMetadataOnly"],
    },
    state: "ready",
    error: "",
  });
  configApp.component("a-button", defineComponent({
    setup(_, { slots }) {
      return () => h("button", slots.default?.());
    },
  }));
  const configHtml = await renderToString(configApp);
  const versionHtml = await renderToString(createSSRApp(versionModule.default, {
    version: null,
    compact: false,
    checking: false,
  }));

  assert.match(configHtml, /此来源仅提供元数据/);
  assert.match(configHtml, /凭据文件仅提供元数据/);
  assert.doesNotMatch(configHtml, /文件缺失或不可读取/);
  assert.match(versionHtml, /未检查/);
  assert.doesNotMatch(versionHtml, /有可用更新/);
});

test("deep scan offers a candidate without changing the configured executable", async () => {
  const centerModule = await server.ssrLoadModule("/src/composables/useAgentEnvironmentCenter.ts");
  const settings = deepScanSettings();
  const { center, app } = mountCenter(centerModule.useAgentEnvironmentCenter, settings);
  const request = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const pending = center.startDeepScan();

  assert.equal(center.deepScanState.value, "scanning");
  request.resolve(deepScanProbe());
  await pending;

  assert.equal(center.deepScanCandidates.value.length, 1);
  assert.equal(center.canAdoptDeepScanCandidate(center.deepScanCandidates.value[0]), true);
  assert.deepEqual(settings.agentCliPaths, {});
  app.unmount();
});

test("deep scan keeps a concurrent path edit and disables only that candidate", async () => {
  const centerModule = await server.ssrLoadModule("/src/composables/useAgentEnvironmentCenter.ts");
  const settings = deepScanSettings();
  const { center, app } = mountCenter(centerModule.useAgentEnvironmentCenter, settings);
  const request = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const pending = center.startDeepScan();

  settings.agentCliPaths.codex = "/manual/bin/codex";
  request.resolve(deepScanProbe());
  await pending;

  const candidate = center.deepScanCandidates.value[0];
  assert.equal(center.deepScanDraftChanged.value, true);
  assert.equal(center.canAdoptDeepScanCandidate(candidate), false);
  assert.equal(settings.agentCliPaths.codex, "/manual/bin/codex");
  app.unmount();
});

test("cancelled or failed deep scans release state without changing settings", async () => {
  const centerModule = await server.ssrLoadModule("/src/composables/useAgentEnvironmentCenter.ts");
  const settings = deepScanSettings();
  const cancelled = mountCenter(centerModule.useAgentEnvironmentCenter, settings);
  const cancelledRequest = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const cancelledPending = cancelled.center.startDeepScan();
  cancelled.center.cancelDeepScan();
  cancelledRequest.resolve(deepScanProbe());
  await cancelledPending;
  assert.equal(cancelled.center.deepScanState.value, "idle");
  assert.deepEqual(settings.agentCliPaths, {});
  cancelled.app.unmount();

  const failed = mountCenter(centerModule.useAgentEnvironmentCenter, settings);
  const failedRequest = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const failedPending = failed.center.startDeepScan();
  failedRequest.reject(new Error("shell unavailable"));
  await failedPending;
  assert.equal(failed.center.deepScanState.value, "error");
  assert.match(failed.center.deepScanError.value, /shell unavailable/);
  assert.deepEqual(settings.agentCliPaths, {});
  failed.app.unmount();
});

test("unmounting the environment center cancels deep scan UI state", async () => {
  const centerModule = await server.ssrLoadModule("/src/composables/useAgentEnvironmentCenter.ts");
  const settings = deepScanSettings();
  const { center, app } = mountCenter(centerModule.useAgentEnvironmentCenter, settings);
  const request = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const pending = center.startDeepScan();
  app.unmount();
  request.resolve(deepScanProbe());
  await pending;

  assert.equal(center.deepScanState.value, "idle");
  assert.deepEqual(center.deepScanCandidates.value, []);
  assert.deepEqual(settings.agentCliPaths, {});
});

test("cancelled CLI probes cannot write a late shared result or keep busy state", async () => {
  const cliModule = await server.ssrLoadModule("/src/stores/cli-runtime.ts");
  setActivePinia(createPinia());
  const cliStore = cliModule.useCliRuntimeStore();
  const request = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const pending = cliStore.probeCliTools(true);
  const requestId = cliStore.cliEnvironmentRequestId;
  assert.equal(cliStore.cliEnvironmentLoading, true);
  cliStore.cancelCliToolsProbe(requestId);
  request.resolve(deepScanProbe());
  await pending;
  assert.equal(cliStore.cliEnvironmentLoading, false);
  assert.equal(cliStore.cliEnvironmentProbe, null);
});

function freshStore() {
  setActivePinia(createPinia());
  commandQueues.clear();
  return storeModule.useAgentEnvironmentStore();
}

function mountCenter(factory: CenterModule["useAgentEnvironmentCenter"], settings: AppSettings) {
  const pinia = createPinia();
  setActivePinia(pinia);
  let center!: ReturnType<CenterModule["useAgentEnvironmentCenter"]>;
  const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: (type) => ({ type }),
    createText: (text) => ({ text }),
    createComment: (text) => ({ text }),
    setText(node, text) { node.text = text; },
    setElementText(node, text) { node.text = text; },
    parentNode: () => null,
    nextSibling: () => null,
    querySelector: () => null,
    setScopeId() {},
    cloneNode: (node) => ({ ...node }),
    insertStaticContent: () => [{}, {}],
  });
  const app = renderer.createApp(defineComponent({
    setup() {
      center = factory({ settings });
      return () => h("div");
    },
  }));
  app.use(pinia);
  app.mount({});
  return { center, app };
}

function deepScanSettings() {
  return {
    agentCliPaths: {},
    livenessCliKind: "codex",
  } as AppSettings;
}

function deepScanProbe(): CliEnvironmentProbeResult {
  return {
    tools: [{
      kind: "codex",
      label: "Codex CLI",
      executable: "codex",
      sessionNameHint: "",
      capabilities: {
        temporaryLaunch: true,
        modelSelection: true,
        sessionHistory: true,
        sessionSearch: true,
        sessionDetail: true,
        sessionResume: true,
        sessionName: false,
        liveness: true,
        defaultConfig: true,
      },
      available: true,
      path: "/login-shell/bin/codex",
      version: "0.1.0",
      message: "",
    }],
  };
}

function queueCommand<T = unknown>(command: string): Deferred<T> {
  const pending = deferred<T>();
  const queue = commandQueues.get(command) ?? [];
  queue.push(pending as Deferred<unknown>);
  commandQueues.set(command, queue);
  return pending;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

function inventoryFixture(scannedAt: string, workspace: string | null = null): AgentEnvironmentInventory {
  return {
    environment: {
      id: "native:macos",
      kind: "native",
      hostPlatform: "macos",
      hostArchitecture: "aarch64",
      guestPlatform: null,
      displayName: "本机 (macos)",
      capabilities: ["readOnlyInventory", "boundedPreview"],
    },
    installations: [installationFixture()],
    sources: [],
    capabilities: [{ agentKind: "codex", assets: [] }],
    assets: [],
    hookRuleCounts: [{ agentKind: "codex", ruleCount: 0 }],
    scannedAt,
    workspace,
    contexts: [{ id: "context:codex:default", environmentId: "native:macos", agentKind: "codex", configRoot: "/tmp", profile: "default", workspaceId: workspace, trustContext: "trusted", parserVersion: 1, schemaFacts: {}, compatibleInstallationIds: ["installation:codex"] }],
    declarations: [],
    mechanisms: [],
    diagnostics: [],
    limits: { candidatePathsPerAgent: 32, installationsPerAgent: 8, sourcesPerContext: 128, firstLevelEntries: 512, bytesPerSource: 524288, bytesPerRefresh: 8388608, refreshBudgetMs: 5000, cliOutputBytes: 262144, cliConcurrency: 2, watchers: 64, diagnostics: 100 },
  };
}

function installationFixture(): AgentInstallation {
  return {
    id: "installation:codex",
    environmentId: "native:macos",
    agentKind: "codex",
    label: "Codex",
    availability: "available",
    executablePath: "/usr/local/bin/codex",
    executableIdentity: { owner: "fixture", canonicalPath: "/usr/local/bin/codex", installationSource: "automatic" },
    executableRevision: "fixture:executable:revision",
    installedVersion: "1.0.0",
    discoverySource: "automatic",
    distribution: "npm",
    channel: "stable",
    installedVersionSource: "localExecutable",
    diagnostics: [],
  };
}

function previewFixture(content: string): AgentAssetReadResult {
  return {
    stableId: "source:config",
    accessId: "access:config",
    sourceRevision: sourceFixture().revision,
    path: "/tmp/config.toml",
    content,
    sizeBytes: content.length,
    modifiedAt: null,
    truncated: false,
    metadataOnly: false,
    diagnostics: [],
  };
}

function sourceFixture(): AgentAssetSource {
  return {
    id: "source:config",
    contextId: "context:codex:default",
    label: "config.toml",
    scope: "user",
    origin: "configEntry",
    environmentId: "native:macos",
    workspaceId: null,
    path: "/tmp/config.toml",
    precedence: 1,
    writable: true,
    sensitive: true,
    sourceKind: "file",
    categories: ["mcp"],
    revision: {
      identity: "revision:config",
      observedAt: "2026-09-05T00:00:00Z",
      sizeBytes: 2,
      isMissing: false,
      isDirectory: false,
      isSymlink: false,
    },
    diagnostics: [],
    access: { kind: "ready", accessId: "access:config" },
    actions: ["inspect", "preview"].map((action) => ({ action: action === "inspect" ? "inspect" : "preview", available: true, reason: null, mechanismId: null, confirmationRequired: false, reloadEffect: null, trustEffect: null, selectedInstallationId: null, risks: [] })),
  };
}

test("asset details expose explicit installation state without legacy boolean", () => {
  const source = read("src/stores/provider-types.ts");
  assert.match(source, /export type AgentAssetInstallState =/);
  assert.match(source, /"installed"[\s\S]*"notInstalled"[\s\S]*"unknown"/);
  const plugin = unionVariant(source, "AgentAssetDetails", "plugin");
  const extension = unionVariant(source, "AgentAssetDetails", "extension");
  assert.match(plugin, /installState: AgentAssetInstallState/);
  assert.match(extension, /installState: AgentAssetInstallState/);
  assert.doesNotMatch(plugin, /installed: boolean/);
  assert.doesNotMatch(extension, /installed: boolean/);
});

test("Agent environment TypeScript tagged unions use the complete camelCase wire contract", () => {
  const types = read("src/stores/provider-types.ts");
  const diagnosticVariants: Record<string, string[]> = {
    truncated: ["limit", "accepted", "observedAtLeast"],
    malformed: ["format", "location"],
    duplicateNativeId: ["category", "nativeId"],
    unknownField: ["fieldPath"],
    symlinkRejected: ["sourceId"],
    budgetExceeded: ["elapsedMs", "budgetMs"],
    readFailed: ["sourceId", "errorKind"],
    invalidNativeId: ["category"],
    unresolvedRelationship: ["relation", "nativeId"],
    invalidProjection: ["projectionKey"],
    invalidResolution: ["projectionKey", "resolution"],
    installationProbeFailed: ["candidateSource", "errorKind"],
    sourceOutsideAllowedRoot: ["sourceId"],
    sourceTypeMismatch: ["sourceId", "expected", "actual"],
    invalidCompatibleInstallation: ["installationId"],
    declarationSuppressed: ["reason"],
    discoveryIncomplete: ["agentKind", "category", "reason"],
    policyBlocked: [],
  };
  const diagnosticLegacyFields: Record<string, string[]> = {
    truncated: ["observed_at_least"],
    duplicateNativeId: ["native_id"],
    unknownField: ["field_path"],
    symlinkRejected: ["source_id"],
    budgetExceeded: ["elapsed_ms", "budget_ms"],
    readFailed: ["source_id", "error_kind"],
    unresolvedRelationship: ["native_id"],
    invalidProjection: ["projection_key"],
    invalidResolution: ["projection_key"],
    installationProbeFailed: ["candidate_source", "error_kind"],
    sourceOutsideAllowedRoot: ["source_id"],
    sourceTypeMismatch: ["source_id"],
    invalidCompatibleInstallation: ["installation_id"],
    discoveryIncomplete: ["agent_kind"],
  };
  assert.deepEqual(Object.keys(diagnosticVariants).sort(), Object.keys(diagnosticLegacyFields).concat([
    "malformed",
    "invalidNativeId",
    "declarationSuppressed",
    "policyBlocked",
  ]).sort());
  for (const [kind, fields] of Object.entries(diagnosticVariants)) {
    const variant = unionVariant(types, "AgentAssetDiagnostic", kind);
    assert.match(variant, new RegExp(`kind: "${kind}"`));
    for (const field of fields) assert.match(variant, new RegExp(`\\b${field}\\s*:`));
    for (const field of diagnosticLegacyFields[kind] ?? []) {
      assert.doesNotMatch(variant, new RegExp(`\\b${field}\\s*:`));
    }
  }

  const policyFields = {
    declaration: ["declarationId"],
    source: ["sourceId"],
  };
  for (const [kind, fields] of Object.entries(policyFields)) {
    const variant = unionVariant(types, "AgentAssetPolicyReference", kind);
    for (const field of fields) assert.match(variant, new RegExp(`\\b${field}\\s*:`));
    assert.doesNotMatch(variant, /declaration_id|source_id/);
  }

  const detailFields = {
    skill: ["enabled", "invocationPolicy"],
    mcp: ["transport", "declaredState", "approvalState", "effectiveAvailability"],
    plugin: ["installState", "enabled", "trusted"],
    extension: ["installState", "enabled", "trusted"],
    hook: ["managed", "enabled", "ruleCount"],
    statusUi: ["mode", "commandPresent"],
  };
  for (const [kind, fields] of Object.entries(detailFields)) {
    const variant = unionVariant(types, "AgentAssetDetails", kind);
    for (const field of fields) assert.match(variant, new RegExp(`\\b${field}\\s*:`));
    for (const field of [
      "invocation_policy",
      "declared_state",
      "approval_state",
      "effective_availability",
      "install_state",
      "command_present",
      "rule_count",
      "installed",
    ]) {
      assert.doesNotMatch(variant, new RegExp(`\\b${field}\\s*:`));
    }
  }
  assert.match(unionVariant(types, "AgentAssetDetails", "hook"), /ruleCount\s*:\s*number\s*\|\s*null/);
  assert.match(types, /interface AgentHookRuleCount\s*\{\s*agentKind:\s*AgentCliKind;\s*ruleCount:\s*number\s*\|\s*null;/);
  assert.match(types, /hookRuleCounts:\s*AgentHookRuleCount\[\]/);
});

test("Agent environment participation unions keep reason only on suppressed", () => {
  const types = read("src/stores/provider-types.ts");
  const participates = unionVariant(
    types,
    "AgentAssetResolutionParticipation",
    "participates",
  );
  const suppressed = unionVariant(
    types,
    "AgentAssetResolutionParticipation",
    "suppressed",
  );

  assert.match(participates, /kind: "participates"/);
  assert.doesNotMatch(participates, /\breason\s*:/);
  assert.match(suppressed, /kind: "suppressed"/);
  assert.match(suppressed, /reason: AgentAssetSuppressionReason/);
});

function unionVariant(source: string, typeName: string, kind: string) {
  const typeStart = source.indexOf(`export type ${typeName} =`);
  assert.notEqual(typeStart, -1, `missing union ${typeName}`);
  const typeEnd = source.indexOf("\nexport ", typeStart + 1);
  const union = source.slice(typeStart, typeEnd === -1 ? undefined : typeEnd);
  const marker = `kind: "${kind}"`;
  const variantStart = union.indexOf(marker);
  assert.notEqual(variantStart, -1, `missing ${typeName}.${kind}`);
  const nextVariantStart = union.indexOf('kind: "', variantStart + marker.length);
  return union.slice(variantStart, nextVariantStart === -1 ? undefined : nextVariantStart);
}

function hookInspectionFixture(state: "healthy" | "not_installed", revision: string) {
  return {
    agentKind: "codex",
    runtimeScope: { kind: "native" },
    configPath: "/tmp/hooks.json",
    configExists: true,
    revision,
    state,
    installed: state === "healthy",
    enabled: state === "healthy",
    trusted: state === "healthy" ? "trusted" : "unknown",
    helperAvailable: true,
    spoolAvailable: true,
    lastEventAt: state === "healthy" ? 1 : null,
    ownership: null,
    diagnostics: [],
    actions: [],
  };
}

function hookPlanFixture() {
  return {
    agentKind: "codex",
    mutation: "install",
    runtimeScope: { kind: "native" },
    configPath: "/tmp/hooks.json",
    expectedRevision: "new",
    supported: true,
    conflict: false,
    changes: [{
      eventName: "SessionStart",
      structuralIdentity: "balancehub:session-start",
      fingerprint: "fingerprint",
      kind: "add",
    }],
    contentChanges: [],
    summary: "install",
  };
}
