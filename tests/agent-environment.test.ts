import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test, { after, before } from "node:test";
import { fileURLToPath } from "node:url";
import { createPinia, setActivePinia } from "pinia";
import { createRenderer, createSSRApp, defineComponent, h } from "vue";
import { renderToString } from "@vue/server-renderer";
import { createServer, type ViteDevServer } from "vite";
import type { AppSettings, CliEnvironmentProbeResult } from "../src/stores/provider-types.ts";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (path: string) => readFileSync(join(root, path), "utf8");
type StoreModule = typeof import("../src/stores/agent-environment.ts");
type HookStoreModule = typeof import("../src/stores/agent-hooks.ts");
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
  server = await createServer({ server: { middlewareMode: true }, appType: "custom", logLevel: "silent" });
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
  assert.match(source, /check_agent_latest_versions/);
  assert.match(source, /read_agent_environment_asset/);
  assert.match(source, /open_agent_environment_asset/);
  assert.doesNotMatch(source, /openPath|open_path/);
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

test("Agent environment uses a dynamic list with explicit row controls", () => {
  const page = read("src/components/settings/SettingsAgentEnvironmentCenter.vue");
  const consoleSource = read("src/components/settings/agent-environment/AgentEnvironmentConsole.vue");
  const row = read("src/components/settings/agent-environment/AgentEnvironmentRow.vue");
  assert.match(page, /AgentEnvironmentConsole/);
  assert.doesNotMatch(page, /AgentInstallationGrid|AgentHookManager/);
  assert.match(consoleSource, /v-for="row in rows"/);
  assert.match(consoleSource, /agentHookTargetKey/);
  assert.doesNotMatch(consoleSource, /codex|claude|gemini|grok/);
  assert.match(row, /<article class="agent-environment-row"/);
  assert.doesNotMatch(row, /<button[\s\S]*agent-environment-row/);
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

test("version state is isolated by workspace and stale checks cannot overwrite it", async () => {
  const store = freshStore();
  const nativeInventory = inventoryFixture("native");
  const workspaceInventory = inventoryFixture("workspace", "/workspace");
  store.inventories.__native__ = nativeInventory;
  store.inventories["/workspace"] = workspaceInventory;

  const oldNative = queueCommand("check_agent_latest_versions");
  const newNative = queueCommand("check_agent_latest_versions");
  const workspace = queueCommand("check_agent_latest_versions");
  const oldPromise = store.refreshLatestVersions();
  const newPromise = store.refreshLatestVersions();
  const workspacePromise = store.refreshLatestVersions("/workspace");

  workspace.resolve(versionFixture("3.0.0", "workspace"));
  newNative.resolve(versionFixture("2.0.0", "new"));
  await Promise.all([workspacePromise, newPromise]);
  oldNative.resolve(versionFixture("1.0.0", "old"));
  await oldPromise;

  assert.equal(store.inventory()?.installations[0].latestStableVersion, "2.0.0");
  assert.equal(store.inventory("/workspace")?.installations[0].latestStableVersion, "3.0.0");
  assert.equal(store.versionState.__native__, "ready");
  assert.equal(store.versionState["/workspace"], "ready");

  const failedCheck = queueCommand("check_agent_latest_versions");
  const failedPromise = store.refreshLatestVersions();
  failedCheck.reject(new Error("registry unavailable"));
  await assert.rejects(failedPromise, /registry unavailable/);
  assert.equal(store.inventory()?.installations[0].latestStableVersion, "2.0.0");
  assert.equal(store.versionState.__native__, "error");
  assert.equal(store.versionErrors.__native__, "registry unavailable");
});

test("a late config preview cannot replace the newer preview for the same asset", async () => {
  const store = freshStore();
  const oldRequest = queueCommand("read_agent_environment_asset");
  const newRequest = queueCommand("read_agent_environment_asset");
  const oldPromise = store.readConfigPreview("asset:config");
  const newPromise = store.readConfigPreview("asset:config");

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
  const asset = assetFixture();
  const previewRequest = queueCommand("read_agent_environment_asset");
  const previewPromise = center!.selectConfig(asset);

  center!.closeInstallation();
  previewRequest.resolve(previewFixture("late preview"));
  await previewPromise;
  assert.equal(center!.configPreview.value, null);
  assert.equal(center!.configPreviewState.value, "idle");
  assert.equal(Object.values(store.previews)[0]?.content, "late preview");
  assert.deepEqual(Object.values(store.previewState), ["ready"]);

  const inventoryRequest = queueCommand("get_agent_environment_inventory");
  const inventoryPromise = center!.loadInventory(true);
  app.unmount();
  inventoryRequest.resolve(inventoryFixture("late inventory"));
  assert.equal(await inventoryPromise, null);
  assert.equal(store.inventories.__native__?.scannedAt, "late inventory");
  assert.equal(store.inventoryState.__native__, "ready");
});

test("Agent environment configuration preview is bounded and read-only", () => {
  const source = read("src/components/settings/agent-environment/AgentConfigBrowser.vue");
  const styles = read("src/styles/modules/agent-environment.css");
  assert.match(source, /内容已截断/);
  assert.match(source, /仅元数据/);
  assert.match(source, /当前文件不提供内容预览/);
  assert.match(styles, /\.agent-config-content[\s\S]*overflow: auto/);
  assert.match(styles, /\.agent-config-preview[\s\S]*min-height: 286px/);
});

test("metadata-only previews and unknown versions render without inventing failure or update state", async () => {
  const configModule = await server.ssrLoadModule(
    "/src/components/settings/agent-environment/AgentConfigBrowser.vue",
  );
  const versionModule = await server.ssrLoadModule(
    "/src/components/settings/agent-environment/AgentVersionStatus.vue",
  );
  const configApp = createSSRApp(configModule.default, {
    files: [assetFixture()],
    supported: true,
    selectedId: "asset:config",
    preview: {
      ...previewFixture(""),
      content: null,
      metadataOnly: true,
      diagnostic: "敏感凭据文件仅提供元数据",
    },
    previewState: "ready",
    previewError: "",
    copiedPathId: null,
  });
  configApp.component("a-button", defineComponent({
    setup(_, { slots }) {
      return () => h("button", slots.default?.());
    },
  }));
  const configHtml = await renderToString(configApp);
  const versionHtml = await renderToString(createSSRApp(versionModule.default, {
    installation: installationFixture(),
    compact: false,
    checking: false,
  }));

  assert.match(configHtml, /当前资产仅提供元数据/);
  assert.match(configHtml, /敏感凭据文件仅提供元数据/);
  assert.doesNotMatch(configHtml, /文件缺失或不可读取/);
  assert.match(versionHtml, /版本未知/);
  assert.doesNotMatch(versionHtml, /有可用更新/);
});

test("deep scan adopts one candidate only after an unchanged settings snapshot", async () => {
  const centerModule = await server.ssrLoadModule("/src/composables/useAgentEnvironmentCenter.ts");
  const settings = deepScanSettings();
  const { center, app } = mountCenter(centerModule.useAgentEnvironmentCenter, settings);
  const request = queueCommand<CliEnvironmentProbeResult>("probe_cli_tools");
  const pending = center.startDeepScan();

  assert.equal(center.deepScanState.value, "scanning");
  request.resolve(deepScanProbe());
  await pending;

  assert.equal(center.deepScanCandidates.value.length, 1);
  assert.equal(center.adoptDeepScanCandidate(center.deepScanCandidates.value[0]), true);
  assert.equal(settings.agentCliPaths.codex, "/login-shell/bin/codex");
  assert.equal(center.isDeepScanCandidateAdopted(center.deepScanCandidates.value[0]), true);
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
  assert.equal(center.adoptDeepScanCandidate(candidate), false);
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
  assert.equal(center.deepScanResult.value, null);
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

function mountCenter(factory: (options: { settings?: AppSettings }) => any, settings: AppSettings) {
  const pinia = createPinia();
  setActivePinia(pinia);
  let center: any;
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

function inventoryFixture(scannedAt: string, workspace: string | null = null) {
  return {
    environment: {
      id: "native:macos",
      kind: "native",
      hostPlatform: "macos",
      guestPlatform: null,
      displayName: "本机 (macos)",
      capabilities: ["readOnlyInventory", "boundedPreview"],
    },
    installations: [installationFixture()],
    sources: [],
    capabilities: [{ agentKind: "codex", assets: [] }],
    assets: [],
    scannedAt,
    workspace,
  };
}

function installationFixture() {
  return {
    id: "installation:codex",
    environmentId: "native:macos",
    agentKind: "codex",
    label: "Codex",
    availability: "available",
    executablePath: "/usr/local/bin/codex",
    installedVersion: "1.0.0",
    discoverySource: "automatic",
    channel: "stable",
    installedVersionSource: "localExecutable",
    latestStableVersion: null,
    latestVersionSource: "unknown",
    versionState: "unknown",
    versionCheckedAt: null,
    diagnostic: null,
  };
}

function versionFixture(version: string, checkedAt: string) {
  return {
    installations: [{
      ...installationFixture(),
      latestStableVersion: version,
      latestVersionSource: "npmRegistry",
      versionState: "updateAvailable",
      versionCheckedAt: checkedAt,
    }],
    checkedAt,
  };
}

function previewFixture(content: string) {
  return {
    stableId: "asset:config",
    path: "/tmp/config.toml",
    content,
    sizeBytes: content.length,
    modifiedAt: null,
    truncated: false,
    metadataOnly: false,
    diagnostic: null,
  };
}

function assetFixture() {
  return {
    stableId: "asset:config",
    agentKind: "codex",
    category: "config",
    nativeId: "config",
    label: "config.toml",
    sourceId: "source:config",
    scope: "user",
    environmentId: "native:macos",
    workspaceId: null,
    path: "/tmp/config.toml",
    precedence: 1,
    writable: true,
    declaredState: "unknown",
    effectiveState: "unknown",
    trustState: null,
    diagnostics: [],
    revision: "revision",
    sensitive: true,
    isDirectory: false,
  };
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
    summary: "install",
  };
}
