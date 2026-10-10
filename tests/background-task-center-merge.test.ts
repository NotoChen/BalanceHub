import assert from "node:assert/strict";
import test, { after, before, type TestContext } from "node:test";
import { createRenderer, defineComponent, h, nextTick, ref } from "vue";
import { createServer, type ViteDevServer } from "vite";
import type { BackgroundTask } from "../src/composables/useBackgroundTaskCenter.ts";
import type { ProviderBrowserLoginTask } from "../src/api/provider-browser-login.ts";
import type { CheckInTask } from "../src/api/checkin.ts";
import type { BrowserRuntimeStatus } from "../src/api/browser-runtime.ts";

type CenterModule = typeof import("../src/composables/useBackgroundTaskCenter.ts");
type CenterOptions = Parameters<CenterModule["useBackgroundTaskCenter"]>[0];
type Center = ReturnType<CenterModule["useBackgroundTaskCenter"]>;
type TaskEvent = Omit<BackgroundTask, "id" | "source" | "actions"> & {
  taskId: string; canCancel?: boolean; canShowWindow?: boolean; loginAccountId?: string; providerId?: string;
};
type Deferred = { promise: Promise<unknown>; resolve: (value?: unknown) => void; reject: (failure: unknown) => void };

let server: ViteDevServer;
let module: CenterModule;
const listeners = new Set<(event: { payload: TaskEvent }) => void>();
const calls: { command: string; args: Record<string, unknown> }[] = [];
const queues = new Map<string, Deferred[]>();
const unexpected: string[] = [];
const messages: string[] = [];

before(async () => {
  Object.defineProperty(globalThis, "window", { value: globalThis, configurable: true });
  Object.defineProperty(globalThis, "__BALANCEHUB_TASK_CENTER_TEST__", { configurable: true, value: {
    messages,
    listen(name: string, receive: (event: { payload: TaskEvent }) => void) {
      assert.equal(name, "background-task"); listeners.add(receive); return () => listeners.delete(receive);
    },
  } });
  Object.defineProperty(globalThis, "__TAURI_INTERNALS__", { configurable: true, value: {
    invoke(command: string, args: Record<string, unknown> = {}) {
      calls.push({ command, args });
      const response = queues.get(command)?.shift();
      if (response) return response.promise;
      if (command === "list_provider_browser_logins") return Promise.resolve([]);
      unexpected.push(command); return Promise.reject(new Error("Unexpected command: " + command));
    },
  } });
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    configFile: false, server: { middlewareMode: true, hmr: false }, appType: "custom", logLevel: "silent",
    resolve: { alias: [
      { find: /^@tauri-apps\/api\/event$/, replacement: "virtual:task-center-events" },
      { find: /^@arco-design\/web-vue$/, replacement: "virtual:task-center-messages" },
    ] },
    plugins: [{ name: "task-center-merge-fixtures", resolveId(id) {
      if (id === "virtual:task-center-events" || id === "virtual:task-center-messages") return "\0" + id.slice("virtual:".length);
    }, load(id) {
      if (id === "\0task-center-events") return "export async function listen(name, receive) { return globalThis.__BALANCEHUB_TASK_CENTER_TEST__.listen(name, receive); }";
      if (id === "\0task-center-messages") return "export const Message = { error(message) { globalThis.__BALANCEHUB_TASK_CENTER_TEST__.messages.push(message); } };";
    } }],
  });
  module = await server.ssrLoadModule("/src/composables/useBackgroundTaskCenter.ts") as CenterModule;
});

after(async () => { await server.close(); });

function pending(command: string): Deferred {
  let resolve!: Deferred["resolve"]; let reject!: Deferred["reject"];
  const promise = new Promise<unknown>((done, fail) => { resolve = done; reject = fail; });
  const deferred = { promise, resolve, reject }; const queue = queues.get(command) ?? [];
  queue.push(deferred); queues.set(command, queue); return deferred;
}
async function settle() { for (let index = 0; index < 24; index += 1) await Promise.resolve(); await nextTick(); }
function emit(event: TaskEvent) { for (const receive of listeners) receive({ payload: event }); }
function event(taskId: string, fields: Partial<TaskEvent> = {}): TaskEvent {
  return { taskId, kind: "providerLogin", status: "waiting", title: "隔离登录任务", detail: "等待用户完成登录",
    progress: null, startedAt: Date.now() - 1_000, canCancel: true, canShowWindow: true, ...fields };
}
function agentTask(id = "agent-catalog-fixture", fields: Partial<BackgroundTask> = {}): BackgroundTask {
  return { id, kind: "sync", title: "Agent 领域任务", detail: "领域仍在执行", status: "running", progress: null,
    startedAt: Date.now() - 1_000, source: "manual", ...fields };
}
function loginTask(runId: string, fields: Partial<ProviderBrowserLoginTask> = {}): ProviderBrowserLoginTask {
  return { runId, providerId: null, providerName: "隔离站点", loginAccountId: "fixture-account", operation: "account",
    phase: "waitingLogin", message: "继续已有登录", startedAt: Date.now() - 1_000, finishedAt: null,
    error: null, canCancel: true, canShowWindow: true, ...fields };
}
function checkInTask(fields: Partial<CheckInTask> = {}): CheckInTask {
  return { runId: "checkin-fixture", providerId: "fixture-provider", providerName: "隔离站点", batchId: null,
    source: "manual", phase: "waitingHuman", message: "等待验证", revision: 1, finished: false, canResume: true,
    canCancel: true, canShowWindow: false, startedAt: Date.now() - 1_000, finishedAt: null, ...fields };
}
function browserRuntime(fields: Partial<BrowserRuntimeStatus> = {}): BrowserRuntimeStatus {
  return { phase: "installing", message: "读取隔离组件", ready: false, installed: false, browser: null, systemBrowsers: [], selection: { mode: "managed" }, runtimeReady: false,
    expectedVersion: "fixture-version", installedVersion: null, progress: null, revision: 1, detectedAt: Date.now(),
    canInstall: false, canUninstall: false, runtimeDownloadBytes: 0, browserDownloadBytes: 0, ...fields };
}

function fresh(t: TestContext) {
  assert.equal(listeners.size, 0); calls.length = 0; unexpected.length = 0; messages.length = 0; queues.clear();
  const opened: string[] = []; const controls: string[] = [];
  const options: CenterOptions = {
    providers: ref([]), openLoginAccount: (id) => opened.push("account:" + id), openProviderCredentials: (id) => opened.push("provider:" + id),
    batchOperation: ref(null), batchOperationRunning: ref(false), batchOperationItems: ref([]), batchOperationError: ref(""), batchOperationCompleted: ref(false),
    refreshInProgress: ref(false), refreshingProviderIds: ref(new Set<string>()), checkInTasks: ref([]), checkInPending: ref([]),
    resumeCheckInTask: async (task) => { controls.push("resume:" + task.runId); }, cancelCheckInTask: async (task) => { controls.push("cancel:" + task.runId); },
    showCheckInWindow: async (task) => { controls.push("show:" + task.runId); },
    browserRuntime: ref(null), cancelBrowserRuntime: async () => { controls.push("cancel:browser"); },
    checkingForUpdate: ref(false), updateCheckError: ref(""), installingUpdate: ref(false), updateDownloadProgress: ref(null), updateInstallStatus: ref(""), updateInstallError: ref(""),
    announcementsLoading: ref(false), announcementFatalError: ref(""), announcementErrors: ref([]), cliRuntimeLoading: ref(false),
    temporaryCliLaunchTasks: ref([]), probingCapabilitiesProviderId: ref(null), domainTasks: ref<BackgroundTask[]>([]),
  };
  const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
    patchProp() {}, insert() {}, remove() {}, createElement: (type) => ({ type }), createText: (text) => ({ text }), createComment: (text) => ({ text }),
    setText(node, text) { node.text = text; }, setElementText(node, text) { node.text = text; }, parentNode: () => null, nextSibling: () => null,
  });
  let center!: Center;
  const app = renderer.createApp(defineComponent({ setup() { center = module.useBackgroundTaskCenter(options); return () => h("div"); } }));
  t.after(async () => { app.unmount(); await settle(); assert.equal(listeners.size, 0); assert.deepEqual(unexpected, []); queues.clear(); });
  return { options, opened, controls, async mount() { app.mount({}); await settle(); return center; } };
}
function action(task: BackgroundTask, label: string) {
  const found = task.actions?.find((item) => item.label === label); assert.ok(found, "Expected action: " + label); return found;
}

test("WebDAV waiting and failure snapshots never create a false success or mark provider cards syncing", async (t) => {
  const context = fresh(t); const center = await context.mount();
  const task = agentTask("cloud-sync-fixture", { kind: "cloudSync", title: "WebDAV 同步", status: "waiting", detail: "等待冲突处理" });
  context.options.domainTasks!.value = [task]; await settle();
  assert.equal(center.activeTaskCount.value, 1);
  assert.equal(context.options.refreshInProgress.value, false);
  assert.equal(context.options.refreshingProviderIds.value.size, 0);
  context.options.domainTasks!.value = [{ ...task, status: "failed", detail: "WebDAV 上传失败", finishedAt: Date.now() }]; await settle();
  assert.equal(center.activeTaskCount.value, 0);
  assert.deepEqual(center.recentTasks.value.map((entry) => entry.status), ["failed"]);
  context.options.domainTasks!.value = [{ ...task, id: "cloud-sync-next", status: "running" }]; await settle();
  context.options.domainTasks!.value = []; await settle();
  assert.deepEqual(center.recentTasks.value.map((entry) => entry.status), ["failed"]);
});

test("waiting login stays active and cancellable until its domain event confirms cancellation", async (t) => {
  const context = fresh(t); const center = await context.mount();
  emit(event("login-waiting")); await settle();
  assert.equal(center.activeTaskCount.value, 1); assert.equal(center.activeTasks.value[0].status, "waiting"); assert.deepEqual(center.recentTasks.value, []);
  const show = pending("show_provider_login_window"); action(center.activeTasks.value[0], "显示登录窗口").run(); show.resolve(); await settle();
  const failed = pending("cancel_provider_browser_login"); const cancel = action(center.activeTasks.value[0], "取消"); cancel.run(); cancel.run(); await settle();
  assert.equal(calls.filter((call) => call.command === "cancel_provider_browser_login").length, 1);
  assert.ok(center.activeTasks.value[0].actions?.every((item) => item.disabled));
  failed.reject(new Error("fixture cancel failed")); await settle();
  assert.deepEqual(messages, ["fixture cancel failed"]); assert.equal(action(center.activeTasks.value[0], "取消").disabled, false);
  const accepted = pending("cancel_provider_browser_login"); action(center.activeTasks.value[0], "取消").run(); accepted.resolve(); await settle();
  assert.equal(center.activeTasks.value[0].status, "waiting"); assert.deepEqual(center.recentTasks.value, []);
  assert.deepEqual(calls.filter((call) => call.command === "cancel_provider_browser_login").map((call) => call.args), [{ runId: "login-waiting" }, { runId: "login-waiting" }]);
  emit(event("login-waiting", { status: "cancelled", detail: "用户已取消登录", finishedAt: Date.now(), canCancel: false, canShowWindow: false })); await settle();
  assert.equal(center.activeTaskCount.value, 0); assert.equal(center.recentTasks.value.length, 1); assert.equal(center.recentTasks.value[0].status, "cancelled");
});

test("login recovery preserves waiting controls and cannot replace a newer terminal event", async (t) => {
  const context = fresh(t); const recovering = pending("list_provider_browser_logins"); const center = await context.mount();
  emit(event("newer-login", { status: "failed", detail: "较新的登录失败", error: "fixture login failed", finishedAt: Date.now(), canCancel: false, canShowWindow: false }));
  recovering.resolve([loginTask("newer-login", { phase: "queued" }), loginTask("recovered-login")]); await settle();
  assert.deepEqual(center.activeTasks.value.map((task) => [task.id, task.status]), [["recovered-login", "waiting"]]);
  assert.deepEqual(center.recentTasks.value.map((task) => [task.id, task.status]), [["newer-login", "failed"]]);
  const cancel = pending("cancel_provider_browser_login"); action(center.activeTasks.value[0], "取消").run(); cancel.resolve(); await settle();
  assert.deepEqual(calls.find((call) => call.command === "cancel_provider_browser_login")?.args, { runId: "recovered-login" });
  emit(event("recovered-login", { status: "success", loginAccountId: "fixture-account", finishedAt: Date.now(), canCancel: false, canShowWindow: false })); await settle();
  action(center.recentTasks.value.find((task) => task.id === "recovered-login")!, "查看登录账号").run();
  emit(event("provider-login", { status: "success", providerId: "fixture-provider", loginAccountId: "fixture-account", finishedAt: Date.now(), canCancel: false, canShowWindow: false })); await settle();
  action(center.recentTasks.value.find((task) => task.id === "provider-login")!, "查看站点凭据").run();
  assert.deepEqual(context.opened, ["account:fixture-account", "provider:fixture-provider"]);
});

test("browser launch validation remains cancellable and records its actual terminal result", async (t) => {
  const context = fresh(t); const center = await context.mount();
  context.options.browserRuntime.value = browserRuntime({ phase: "checking", message: "正在验证浏览器启动" });
  await settle();
  assert.equal(center.activeTaskCount.value, 1);
  assert.equal(center.activeTasks.value[0].title, "验证浏览器");
  action(center.activeTasks.value[0], "取消").run();
  assert.deepEqual(context.controls, ["cancel:browser"]);
  assert.equal(center.activeTaskCount.value, 1, "cancel acknowledgement must not invent a completed operation");
  context.options.browserRuntime.value = browserRuntime({ phase: "cancelled", message: "操作已取消，原有浏览器选择和组件保留", revision: 2 });
  await settle();
  assert.equal(center.activeTaskCount.value, 0);
  assert.equal(center.recentTasks.value[0].status, "cancelled");
  context.options.browserRuntime.value = browserRuntime({ phase: "checking", revision: 3 });
  await settle();
  context.options.browserRuntime.value = browserRuntime({ phase: "failed", message: "浏览器启动失败", revision: 4 });
  await settle();
  assert.equal(center.activeTaskCount.value, 0);
  assert.equal(center.recentTasks.value[0].status, "failed");
  assert.equal(center.recentTasks.value[0].detail, "浏览器启动失败");
});

test("Agent snapshots replace same-ID events and remain the terminal authority after late events", async (t) => {
  const context = fresh(t); const center = await context.mount(); const running = agentTask();
  emit(event(running.id, { kind: "sync", status: "running", title: "通用事件任务" })); await settle();
  context.options.domainTasks!.value = [running]; await settle();
  assert.equal(center.activeTaskCount.value, 1); assert.equal(center.activeTasks.value[0].title, running.title);
  const failed = { ...running, status: "failed" as const, detail: "领域核验失败", finishedAt: Date.now() };
  context.options.domainTasks!.value = [failed]; await settle();
  assert.equal(center.activeTaskCount.value, 0); assert.deepEqual(center.recentTasks.value.map((task) => [task.id, task.status, task.detail]), [[running.id, "failed", failed.detail]]);
  emit(event(running.id, { kind: "sync", status: "success", detail: "晚到的通用成功", finishedAt: Date.now() })); await settle();
  emit(event(running.id, { kind: "sync", status: "waiting", detail: "晚到的通用等待" })); await settle();
  assert.equal(center.activeTaskCount.value, 0); assert.deepEqual(center.recentTasks.value.map((task) => [task.id, task.status, task.detail]), [[running.id, "failed", failed.detail]]);
});

test("an arriving Agent snapshot supersedes an earlier event result without duplicate history or resurrection", async (t) => {
  const context = fresh(t); const center = await context.mount(); const running = agentTask();
  emit(event(running.id, { kind: "sync", status: "success", finishedAt: Date.now() })); await settle();
  assert.equal(center.recentTasks.value.length, 1);
  context.options.domainTasks!.value = [running]; await settle();
  assert.equal(center.activeTaskCount.value, 1); assert.deepEqual(center.recentTasks.value, []);
  emit(event(running.id, { kind: "sync", status: "running" })); await settle();
  context.options.domainTasks!.value = []; await settle();
  assert.equal(center.activeTaskCount.value, 0); assert.deepEqual(center.recentTasks.value, []);
});

test("check-in, browser runtime, CLI launch and Agent results retain their own terminal outcomes", async (t) => {
  const context = fresh(t); const center = await context.mount();
  context.options.checkInTasks.value = [checkInTask()]; context.options.browserRuntime.value = browserRuntime();
  context.options.temporaryCliLaunchTasks.value = [{ id: "temporary-cli-launch-fixture", title: "隔离启动", detail: "正在启动", status: "running", startedAt: Date.now() - 1_000 }];
  context.options.domainTasks!.value = [agentTask()]; await settle();
  assert.equal(center.activeTaskCount.value, 4);
  const checkIn = center.activeTasks.value.find((task) => task.id === "checkin-fixture")!;
  assert.equal(checkIn.status, "waiting"); action(checkIn, "继续验证").run(); action(checkIn, "取消").run();
  action(center.activeTasks.value.find((task) => task.id === "browser-runtime-install")!, "取消").run();
  assert.deepEqual(context.controls, ["resume:checkin-fixture", "cancel:checkin-fixture", "cancel:browser"]);
  context.options.checkInPending.value = ["resume:checkin-fixture"]; await settle();
  assert.ok(center.activeTasks.value.find((task) => task.id === "checkin-fixture")!.actions?.every((item) => item.disabled));
  context.options.checkInTasks.value = [checkInTask({ phase: "unconfirmed", message: "签到结果未确认", finished: true, finishedAt: Date.now(), canResume: false, canCancel: false, revision: 2 })];
  context.options.browserRuntime.value = browserRuntime({ phase: "cancelled", message: "组件安装已取消", revision: 2 });
  context.options.temporaryCliLaunchTasks.value = [{ ...context.options.temporaryCliLaunchTasks.value[0], status: "failed", detail: "启动失败", error: "fixture launch failed", finishedAt: Date.now() }];
  context.options.domainTasks!.value = [agentTask(undefined, { status: "failed", detail: "领域任务失败", finishedAt: Date.now() })]; await settle();
  assert.equal(center.activeTaskCount.value, 0);
  assert.deepEqual(Object.fromEntries(center.recentTasks.value.map((task) => [task.id, [task.status, task.detail]])), {
    "agent-catalog-fixture": ["failed", "领域任务失败"], "browser-runtime-install": ["cancelled", "组件安装已取消"],
    "checkin-fixture": ["unconfirmed", "签到结果未确认"], "temporary-cli-launch-fixture": ["failed", "启动失败"],
  });
});

test("scheduler and vanished domain tasks are not automatically recorded as successful", async (t) => {
  const context = fresh(t); const center = await context.mount();
  context.options.checkInTasks.value = [checkInTask()]; context.options.domainTasks!.value = [agentTask()];
  emit(event("scheduler-fixture", { kind: "autoRefresh", status: "running" })); await settle();
  context.options.checkInTasks.value = []; context.options.domainTasks!.value = [];
  emit(event("scheduler-fixture", { kind: "autoRefresh", status: "failed", detail: "自动刷新失败", finishedAt: Date.now() })); await settle();
  assert.equal(center.activeTaskCount.value, 0);
  assert.deepEqual(center.recentTasks.value.map((task) => [task.id, task.status, task.detail]), [["scheduler-fixture", "failed", "自动刷新失败"]]);
});

test("check-in human waits show the existing window instead of offering another submission", async (t) => {
  const context = fresh(t); const center = await context.mount();
  context.options.checkInTasks.value = [checkInTask({ source: "batch", canResume: false, canShowWindow: true })];
  await settle();
  const live = center.activeTasks.value.find((task) => task.id === "checkin-fixture")!;
  assert.equal(live.status, "waiting");
  assert.deepEqual(live.actions?.map((item) => item.label), ["显示签到窗口", "取消"]);
  action(live, "显示签到窗口").run();
  assert.deepEqual(context.controls, ["show:checkin-fixture"]);
  context.options.checkInPending.value = ["show:checkin-fixture"];
  await settle();
  assert.ok(center.activeTasks.value[0].actions?.every((item) => item.disabled));
});
