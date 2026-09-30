import assert from "node:assert/strict";
import test, { after, before, type TestContext } from "node:test";
import { createPinia, disposePinia, setActivePinia, type Pinia } from "pinia";
import { createRenderer, createSSRApp, defineComponent, effectScope, h, ref, type Component } from "vue";
import { renderToString } from "@vue/server-renderer";
import { createServer, type ViteDevServer } from "vite";
import type { AgentSessionResumeRequest, AgentSessionRow } from "../src/stores/agent-session-types.ts";
import type { CliEnvironmentProbeResult, Provider, TemporaryCliPreference, TerminalEnvironmentProbeResult } from "../src/stores/provider-types.ts";
import { resumeOperation, runtimeSession, sessionInstance, sessionRow } from "./agent-session-fixtures.ts";
import { workspaceAgent } from "./agent-workspace-fixtures.ts";

type StoreModule = typeof import("../src/stores/agent-session-resume.ts");
type ResumeModule = typeof import("../src/composables/useAgentSessionResume.ts");
type WorkspaceModule = typeof import("../src/stores/workspaces.ts");
type CliModule = typeof import("../src/stores/cli-runtime.ts");
type Deferred = { promise: Promise<unknown>; resolve: (value: unknown) => void; reject: (error: unknown) => void };
let server: ViteDevServer;
let stores: StoreModule;
let resumes: ResumeModule;
let workspaceStores: WorkspaceModule;
let cliStores: CliModule;
const calls: { command: string; args: Record<string, unknown> }[] = [];
const queues = new Map<string, Deferred[]>();
const unexpected: string[] = [];
let observe: ((command: string) => void) | null = null;
const cliProbe: CliEnvironmentProbeResult = { tools: [{ ...workspaceAgent(), available: true, path: "/fixture/bin/codex", version: "1.0.0", message: "" }] };
const terminalProbe: TerminalEnvironmentProbeResult = { terminals: [{ kind: "terminal", name: "系统终端", available: true, version: "1.0.0", message: "" }] };

before(async () => {
  Object.defineProperty(globalThis, "window", { configurable: true, value: globalThis });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", { configurable: true, value: {
    invoke(command: string, args: Record<string, unknown> = {}) {
      calls.push({ command, args });
      observe?.(command);
      const reply = queues.get(command)?.shift();
      if (reply) return reply.promise;
      if (["list_agent_session_resume_operations", "list_agent_configuration_operations"].includes(command)) return Promise.resolve([]);
      if (command === "probe_cli_tools") return Promise.resolve(structuredClone(cliProbe));
      if (command === "probe_terminals") return Promise.resolve(structuredClone(terminalProbe));
      unexpected.push(command);
      return Promise.reject(new Error(`Unexpected Tauri command: ${command}`));
    },
  } });
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { middlewareMode: true }, appType: "custom", logLevel: "silent",
    resolve: { alias: [{ find: /^@arco-design\/web-vue$/, replacement: "virtual:session-messages" }] },
    plugins: [{ name: "session-messages", resolveId(id) { if (id === "virtual:session-messages") return "\0session-messages"; },
      load(id) { if (id === "\0session-messages") return "export const Message = { success() {}, error() {}, info() {}, warning() {} };"; } }],
  });
  [stores, resumes, workspaceStores, cliStores] = await Promise.all([
    server.ssrLoadModule("/src/stores/agent-session-resume.ts"), server.ssrLoadModule("/src/composables/useAgentSessionResume.ts"),
    server.ssrLoadModule("/src/stores/workspaces.ts"), server.ssrLoadModule("/src/stores/cli-runtime.ts"),
  ]);
});
after(async () => { await server?.close(); });

function fresh(t: TestContext) {
  calls.length = 0; unexpected.length = 0; queues.clear(); observe = null;
  const pinia = createPinia(); setActivePinia(pinia);
  const store = stores.useAgentSessionResumeStore();
  const workspaces = workspaceStores.useWorkspaceStore();
  const cli = cliStores.useCliRuntimeStore();
  cli.cliEnvironmentProbe = structuredClone(cliProbe);
  cli.terminalEnvironmentProbe = structuredClone(terminalProbe);
  const scopes: ReturnType<typeof effectScope>[] = [];
  const unmounts: (() => void)[] = [];
  function mount<T>(setup: () => T) {
    let value!: T;
    const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
      patchProp() {}, insert() {}, remove() {}, createElement: (type) => ({ type }), createText: (text) => ({ text }), createComment: (text) => ({ text }),
      setText(node, text) { node.text = text; }, setElementText(node, text) { node.text = text; }, parentNode: () => null, nextSibling: () => null,
      querySelector: () => null, setScopeId() {}, cloneNode: (node) => ({ ...node }), insertStaticContent: () => [{}, {}],
    });
    const app = renderer.createApp(defineComponent({ setup() { value = setup(); return () => h("div"); } }));
    app.use(pinia); app.mount({}); unmounts.push(() => app.unmount());
    return value;
  }
  function ui(rowValue = sessionRow(), providers: Provider[] = []) {
    const visible = ref(false);
    const row = ref<AgentSessionRow | null>(rowValue);
    const scope = effectScope(); scopes.push(scope);
    const model = scope.run(() => resumes.useAgentSessionResume({ visible, row, scopeRevision: ref("scope:fixture"),
      providers: ref(providers), installations: ref([]), close: () => { visible.value = false; } }))!;
    return { visible, row, model };
  }
  t.after(async () => {
    unmounts.forEach((unmount) => unmount()); scopes.forEach((scope) => scope.stop()); disposePinia(pinia); observe = null; await settle();
    assert.deepEqual(unexpected, []); queues.clear();
  });
  return { pinia, store, workspaces, cli, ui, mount };
}

test("native resume closes before pending IPC, shares reservation, and preserves Provider preferences", async (t) => {
  const context = fresh(t);
  const preference: TemporaryCliPreference = { providerId: "fixture-provider", cliKind: "codex", apiKeyLocalId: "fixture-key", model: "fixture-model", workspacePath: "/fixture/previous" };
  context.workspaces.temporaryCliPreferences = [preference];
  const ui = context.ui(); ui.visible.value = true; await settle();
  const reply = pending("resume_agent_session");
  observe = (command) => { if (command === "resume_agent_session") assert.equal(ui.visible.value, false); };
  assert.equal(ui.model.canConfirm.value, true);
  ui.model.confirm(); ui.model.confirm();
  assert.equal(ui.visible.value, false);
  assert.equal(context.store.starting["ref:session-1"], true);
  assert.equal(context.store.isReserved("ref:session-1"), true);
  const request = submitted();
  assert.deepEqual(Object.keys(request).sort(), ["cliPath", "intent", "requestId", "scopeRevision", "sessionRef", "terminalKind"]);
  assert.deepEqual(request.intent, { kind: "native" });
  assert.equal(request.cliPath, "/fixture/bin/codex");
  assert.equal(request.scopeRevision, "scope:fixture");
  assert.ok(request.requestId);
  await context.store.submit(input());
  assert.equal(resumeCalls().length, 1);
  reply.resolve(resumeOperation({ requestId: request.requestId, state: "succeeded", canCancel: false,
    result: { instance: sessionInstance(), workspaces: [{ path: "/fixture/home", useCount: 2 }], workspaceError: null, preference: null, reused: false } }));
  await settle();
  assert.equal(context.store.starting[request.sessionRef], false);
  assert.equal(context.store.operations["operation:1"].state, "succeeded");
  assert.equal(ui.visible.value, false);
  assert.deepEqual(context.workspaces.workspaces, [{ path: "/fixture/home", useCount: 2 }]);
  assert.deepEqual(context.workspaces.temporaryCliPreferences, [preference]);
});

test("Provider continuation sends only stable Provider and local Key IDs", async (t) => {
  const context = fresh(t);
  const provider = { identity: { id: "fixture-provider" }, auth: { apiKey: "fixture-secret", apiKeyOptions: [{ localId: "fixture-local-key", key: "fixture-secret", keyAvailable: true }] } } as Provider;
  const ui = context.ui(sessionRow(), [provider]); ui.visible.value = true; await settle();
  ui.model.intentKind.value = "provider";
  ui.model.selectProvider("fixture-provider");
  ui.model.apiKeyLocalId.value = "fixture-local-key";
  const reply = pending("resume_agent_session");
  ui.model.confirm();
  const request = submitted();
  assert.deepEqual(request.intent, { kind: "provider", providerId: "fixture-provider", apiKeyLocalId: "fixture-local-key" });
  assert.ok(!JSON.stringify(request).includes("fixture-secret"));
  reply.resolve(resumeOperation({ requestId: request.requestId, state: "failed", canCancel: false }));
  await settle();
  assert.equal(ui.visible.value, false);
  assert.equal(context.store.starting[request.sessionRef], false);
});

test("submission failure releases busy state without automatically replaying the launch", async (t) => {
  const context = fresh(t);
  const reply = pending("resume_agent_session");
  const submitting = context.store.submit(input());
  reply.reject(new Error("fixture launch failed"));
  assert.equal(await submitting, null);
  await settle();
  assert.equal(context.store.starting["ref:session-1"], false);
  assert.equal(context.store.isReserved("ref:session-1"), false);
  assert.match(context.store.startErrors["ref:session-1"], /fixture launch failed/);
  assert.equal(resumeCalls().length, 1);
});

test("submission timeout stays reserved without replay and accepts late acknowledgement in the persistent store", async (t) => {
  const context = fresh(t);
  const reply = pending("resume_agent_session");
  await fastTimeout(15_000, () => context.store.submit(input()));
  const request = submitted();
  assert.equal(context.store.starting[request.sessionRef], false);
  assert.equal(context.store.unconfirmed[request.sessionRef], request.requestId);
  assert.match(context.store.startErrors[request.sessionRef], /不会自动重试/);
  await context.store.submit(input());
  assert.equal(resumeCalls().length, 1);
  reply.resolve(resumeOperation({ requestId: request.requestId, revision: 3, state: "uncertain", canCancel: false }));
  await settle();
  assert.equal(context.store.unconfirmed[request.sessionRef], undefined);
  assert.equal(context.store.operations["operation:1"].revision, 3);
  assert.equal(context.store.isReserved(request.sessionRef), true);
  assert.equal(resumeCalls().length, 1);
});

test("recovery cannot downgrade an operation or replace its native source identity", async (t) => {
  const context = fresh(t);
  context.store.track(resumeOperation({ revision: 5, state: "succeeded", canCancel: false }));
  const reply = pending("list_agent_session_resume_operations");
  const recovering = context.store.recover();
  reply.resolve([resumeOperation({ revision: 4, state: "failed", canCancel: false })]);
  await recovering;
  assert.equal(context.store.operations["operation:1"].revision, 5);
  assert.equal(context.store.operations["operation:1"].state, "succeeded");
  assert.throws(() => context.store.track(resumeOperation({ revision: 6, sessionRef: "ref:different-source", state: "failed" })), /目标不一致/);
});

test("the latest resume operation is selected by its RFC3339 instant across timezone offsets", (t) => {
  const { store } = fresh(t);
  store.track(resumeOperation({ id: "operation:older", state: "failed", canCancel: false, updatedAt: "2026-09-17T08:05:00+08:00" }));
  store.track(resumeOperation({ id: "operation:newer", state: "succeeded", canCancel: false, updatedAt: "2026-09-17T00:10:00Z" }));
  assert.equal(store.operationFor("ref:session-1")?.id, "operation:newer");
  assert.equal(store.operationFor("ref:another-source"), null);
});

test("manual recovery applies the final uncertain resume result once without replaying the launch", async (t) => {
  const context = fresh(t);
  context.workspaces.workspaces = [{ path: "/fixture/previous", useCount: 1 }];
  const reply = pending("resume_agent_session");
  const submitting = context.store.submit(input());
  const request = submitted();
  reply.resolve(resumeOperation({ requestId: request.requestId, revision: 2, state: "uncertain", canCancel: false }));
  await submitting;
  await context.store.submit(input());
  assert.equal(context.store.isReserved(request.sessionRef), true);
  assert.deepEqual(context.workspaces.workspaces, [{ path: "/fixture/previous", useCount: 1 }]);
  assert.equal(resumeCalls().length, 1);
  const completed = resumeOperation({ requestId: request.requestId, revision: 3, state: "succeeded", canCancel: false,
    result: { instance: sessionInstance(), workspaces: [{ path: "/fixture/home", useCount: 4 }], workspaceError: null, preference: null, reused: false } });
  const recovery = pending("list_agent_session_resume_operations");
  const recovering = context.store.recover(); recovery.resolve([completed]); await recovering;
  assert.equal(context.store.isReserved(request.sessionRef), false);
  assert.deepEqual(context.workspaces.workspaces, [{ path: "/fixture/home", useCount: 4 }]);
  context.workspaces.workspaces = [{ path: "/fixture/later-launch", useCount: 1 }];
  const repeated = pending("list_agent_session_resume_operations");
  const recoveringAgain = context.store.recover(); repeated.resolve([{ ...completed, revision: 4 }]); await recoveringAgain;
  assert.deepEqual(context.workspaces.workspaces, [{ path: "/fixture/later-launch", useCount: 1 }]);
  assert.equal(resumeCalls().length, 1);
  assert.equal(calls.filter((call) => call.command === "get_agent_session_resume_operation").length, 0);
});

test("completed session tasks remain visible in the shared recent task center with valid timestamps", async (t) => {
  const context = fresh(t);
  const [backgrounds, centers] = await Promise.all([
    server.ssrLoadModule("/src/composables/useAgentBackgroundTasks.ts") as Promise<typeof import("../src/composables/useAgentBackgroundTasks.ts")>,
    server.ssrLoadModule("/src/composables/useBackgroundTaskCenter.ts") as Promise<typeof import("../src/composables/useBackgroundTaskCenter.ts")>,
  ]);
  const now = Date.now();
  const completed = resumeOperation({ state: "succeeded", canCancel: false, message: "原生会话已继续",
    createdAt: new Date(now - 2_000).toISOString(), updatedAt: new Date(now).toISOString() });
  context.store.track(completed);
  context.store.track(resumeOperation({ id: "operation:expired", sessionRef: "ref:expired", state: "failed", canCancel: false,
    createdAt: new Date(now - 20 * 60_000).toISOString(), updatedAt: new Date(now - 16 * 60_000).toISOString() }));
  const center = context.mount(() => centers.useBackgroundTaskCenter({
    providers: ref([]), batchOperation: ref(null), batchOperationRunning: ref(false), batchOperationItems: ref([]), batchOperationError: ref(""), batchOperationCompleted: ref(false),
    openLoginAccount: () => {}, openProviderCredentials: () => {},
    refreshInProgress: ref(false), refreshingProviderIds: ref(new Set<string>()), checkInTasks: ref([]), checkInPending: ref([]),
    resumeCheckInTask: async () => {}, cancelCheckInTask: async () => {}, browserRuntime: ref(null), cancelBrowserRuntime: async () => {},
    checkingForUpdate: ref(false), updateCheckError: ref(""), installingUpdate: ref(false), updateDownloadProgress: ref(null), updateInstallStatus: ref(""), updateInstallError: ref(""),
    announcementsLoading: ref(false), announcementFatalError: ref(""), announcementErrors: ref([]), cliRuntimeLoading: ref(false), temporaryCliLaunchTasks: ref([]),
    probingCapabilitiesProviderId: ref(null), agentTasks: backgrounds.useAgentBackgroundTasks(),
  }));
  await settle();
  assert.equal(center.activeTaskCount.value, 0);
  assert.equal(center.recentTasks.value.length, 1);
  assert.equal(center.recentTasks.value[0].id, "agent-session-resume-operation:1");
  assert.equal(center.recentTasks.value[0].status, "success");
  assert.equal(center.recentTasks.value[0].startedAt, now - 2_000);
  assert.equal(center.recentTasks.value[0].finishedAt, now);
  context.store.track({ ...completed, revision: 2, message: "继续结果已确认" });
  await settle();
  assert.equal(center.recentTasks.value.length, 1);
  assert.equal(center.recentTasks.value[0].detail, "继续结果已确认");
});

test("closing during environment detection releases probe state and ignores late paths", async (t) => {
  const context = fresh(t);
  const cliReply = pending("probe_cli_tools");
  const terminalReply = pending("probe_terminals");
  const ui = context.ui(); ui.visible.value = true;
  assert.equal(ui.model.probing.value, true);
  ui.visible.value = false;
  assert.equal(ui.model.probing.value, false);
  assert.equal(context.cli.cliEnvironmentLoading, false);
  assert.equal(context.cli.terminalEnvironmentLoading, false);
  cliReply.resolve({ tools: [{ ...cliProbe.tools[0], path: "/fixture/late/codex" }] });
  terminalReply.resolve({ ...terminalProbe, terminals: [] });
  await settle();
  assert.equal(context.cli.cliEnvironmentProbe?.tools[0].path, "/fixture/bin/codex");
  assert.equal(context.cli.terminalEnvironmentProbe?.terminals.length, 1);
  assert.equal(ui.visible.value, false);
});

test("real resume modal remains closable and describes native continuation without a model promise", async (t) => {
  const context = fresh(t);
  const row = sessionRow(); row.session.canResume = false; row.resumeReason = "原生子会话只读";
  const html = await renderSurface("/src/components/agent-workspace/AgentSessionResumeModal.vue", {
    visible: true, row, scopeRevision: "scope:fixture", providers: [], installations: [], agents: [workspaceAgent()],
  }, context.pinia);
  assert.match(html, /data-closable="true"/);
  assert.match(html, /data-mask-closable="true"/);
  assert.match(html, /data-esc-to-close="true"/);
  assert.match(html, /使用原生配置继续原会话，无需选择中转站。/);
  assert.doesNotMatch(html, /原会话模型/);
  assert.match(html, /原生子会话只读/);
  assert.ok(hasDisabledButton(html, "确认继续"));
  await settle();
});

test("real history rows render subagent relationships and only backend-authorized terminal activation", async (t) => {
  const context = fresh(t);
  const row = sessionRow("child", { role: "subagent", parent: { kind: "known", nativeId: "parent", parentRef: "ref:parent" }, runtimeIds: ["exact-terminal", "unknown-terminal"], resumeReason: "该子会话只读" });
  row.session.canResume = false;
  const exact = runtimeSession("exact-terminal", "codex", { actions: { canActivateTerminal: true, canViewDetail: true, canResume: false, canDismiss: false } });
  const unknown = runtimeSession("unknown-terminal");
  const html = await renderSurface("/src/components/agent-workspace/AgentSessionList.vue", {
    rows: [row], agents: [workspaceAgent()], allowResume: true, runtimeSessions: [exact, unknown],
  }, context.pinia);
  assert.match(html, /子会话/);
  assert.match(html, /父会话 parent/);
  assert.match(html, /该子会话只读/);
  assert.equal((html.match(/切回终端/g) ?? []).length, 1);
  assert.ok(hasDisabledButton(html, "继续会话"));
});

async function renderSurface(path: string, props: Record<string, unknown>, pinia: Pinia) {
  const component = (await server.ssrLoadModule(path)).default as Component;
  const app = createSSRApp(component, props); app.use(pinia);
  app.component("a-modal", defineComponent({
    props: { visible: Boolean, closable: Boolean, maskClosable: Boolean, escToClose: Boolean },
    setup(props, { slots }) { return () => props.visible ? h("section", { "data-closable": props.closable, "data-mask-closable": props.maskClosable, "data-esc-to-close": props.escToClose }, [slots.title?.(), slots.default?.()]) : null; },
  }));
  for (const [name, tag] of [["a-button", "button"], ["a-select", "select"], ["a-option", "option"], ["a-tooltip", "span"]]) {
    app.component(name, defineComponent({ setup(_, { attrs, slots }) { return () => h(tag, attrs, slots.default?.()); } }));
  }
  return renderToString(app);
}
function input(): Omit<AgentSessionResumeRequest, "requestId"> {
  return { sessionRef: "ref:session-1", scopeRevision: "scope:fixture", cliPath: "/fixture/bin/codex", terminalKind: "terminal", intent: { kind: "native" } };
}
function pending(command: string) {
  let resolve!: Deferred["resolve"]; let reject!: Deferred["reject"];
  const reply = { promise: new Promise<unknown>((done, fail) => { resolve = done; reject = fail; }), resolve: (value: unknown) => resolve(value), reject: (error: unknown) => reject(error) };
  queues.set(command, [...(queues.get(command) ?? []), reply]); return reply;
}
function resumeCalls() { return calls.filter((call) => call.command === "resume_agent_session"); }
function submitted() { const call = resumeCalls().at(-1); assert.ok(call); return call.args.request as AgentSessionResumeRequest; }
function hasDisabledButton(html: string, label: string) {
  return [...html.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)].some((match) =>
    /\bdisabled\b/.test(match[1]) && match[2].replace(/<[^>]*>/g, "").trim() === label,
  );
}
async function settle() { for (let index = 0; index < 24; index += 1) await Promise.resolve(); }
const originalTimeout = globalThis.setTimeout;
async function fastTimeout<T>(milliseconds: number, action: () => Promise<T>) {
  globalThis.setTimeout = ((callback: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) =>
    originalTimeout(callback, delay === milliseconds ? 0 : delay, ...args)) as typeof setTimeout;
  try { return await action(); } finally { globalThis.setTimeout = originalTimeout; }
}
