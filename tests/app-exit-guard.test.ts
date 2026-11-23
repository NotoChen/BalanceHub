import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { createRenderer, createSSRApp, defineComponent, h, nextTick, ref, type Component } from "vue";
import { compileScript, parse } from "vue/compiler-sfc";
import { renderToString } from "vue/server-renderer";
import ts from "typescript";
import type { BackgroundTask } from "../src/composables/useBackgroundTaskCenter.ts";
import { exitBlockingTasks, exitTaskSummary } from "../src/utils/app-exit.ts";

const componentPath = fileURLToPath(
  new URL("../src/components/AppExitConfirmModal.vue", import.meta.url),
);
const desktopPath = fileURLToPath(
  new URL("../src-tauri/src/desktop.rs", import.meta.url),
);
const guardPath = fileURLToPath(
  new URL("../src/composables/useAppExitGuard.ts", import.meta.url),
);

function task(
  id: string,
  kind: BackgroundTask["kind"],
  overrides: Partial<BackgroundTask> = {},
): BackgroundTask {
  return {
    id,
    kind,
    title: id,
    detail: `${id} 详情`,
    status: "running",
    progress: null,
    startedAt: 1,
    source: "manual",
    ...overrides,
  };
}

test("read-only probes do not trigger the exit confirmation", () => {
  const tasks = [
    task("site-announcements", "announcement"),
    task("cli-runtime-probe", "cliProbe"),
    task("update-check", "update"),
    // 事件流断线只是状态提示，任务本身没有可等待的完成动作。
    task("oauth:connection", "providerLogin", { status: "unconfirmed" }),
    task("agent-lifecycle-connection", "update", { status: "unconfirmed" }),
    task("manual-batch-operation", "refresh"),
    task("checkin-1", "checkIn"),
    task("temporary-cli-launch-1", "cliLaunch"),
    // 自动维护由消费方标记 ambient；同 kind 的 Agent 任务没有标记，仍会拦退出。
    task("auto-refresh", "autoRefresh", { ambient: true }),
    task("provider-refresh", "sync", { ambient: true }),
    task("custom-agent-task", "autoLiveness"),
  ];

  assert.deepEqual(
    exitBlockingTasks(tasks).map((item) => item.id),
    [
      "manual-batch-operation",
      "checkin-1",
      "temporary-cli-launch-1",
      "custom-agent-task",
    ],
  );
  assert.equal(exitTaskSummary([]), "当前没有运行中的任务");
  assert.equal(exitTaskSummary(tasks), "11 个任务仍在运行");
});

let guard: Component | undefined;
function loadGuard(): Component {
  if (guard) return guard;
  const { descriptor } = parse(readFileSync(componentPath, "utf8"), {
    filename: componentPath,
  });
  const compiled = compileScript(descriptor, {
    id: "app-exit-confirm-test",
    inlineTemplate: true,
  });
  const output = ts.transpileModule(compiled.content, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports: { default?: Component } = {};
  const require = createRequire(import.meta.url);
  new Function("require", "exports", output)((specifier: string) => {
    if (specifier === "@lucide/vue") {
      return {
        CircleAlert: defineComponent({ render: () => h("span") }),
        LogOut: defineComponent({ render: () => h("span") }),
        X: defineComponent({ render: () => h("span") }),
      };
    }
    if (specifier === "../utils/app-exit") return { exitTaskSummary };
    return require(specifier);
  }, exports);
  assert.ok(exports.default);
  guard = exports.default;
  return guard;
}

function renderGuard(tasks: BackgroundTask[], settingsPending = false) {
  const app = createSSRApp(loadGuard(), { visible: true, tasks, settingsPending });
  app.component("a-modal", defineComponent({
    setup(_props, { slots }) {
      return () => h("div", [slots.title?.(), slots.default?.()]);
    },
  }));
  app.component("a-button", defineComponent({
    setup(_props, { slots }) {
      return () => h("button", [slots.icon?.(), slots.default?.()]);
    },
  }));
  return renderToString(app);
}

test("exit confirmation lists running targets and caps the visible rows", async () => {
  const tasks = [
    task("manual-batch-operation", "refresh", { title: "全局刷新" }),
    task("checkin-1", "checkIn", { title: "签到 · 站点 A" }),
    task("temporary-cli-launch-1", "cliLaunch", { title: "临时 CLI · Claude" }),
    task("agent-1", "autoLiveness"),
    task("agent-2", "autoLiveness"),
    task("agent-3", "autoLiveness"),
    task("agent-4", "autoLiveness"),
  ];
  const rendered = await renderGuard(tasks);

  assert.match(rendered, /退出 BalanceHub？/);
  assert.match(rendered, /7 个任务仍在运行/);
  assert.match(rendered, /全局刷新/);
  assert.match(rendered, /签到 · 站点 A/);
  assert.match(rendered, /临时 CLI · Claude/);
  assert.match(rendered, /还有 1 个任务未展开/);
  assert.match(rendered, /继续运行/);
  assert.match(rendered, /仍然退出/);
});

test("exit confirmation explains unsaved settings even without running tasks", async () => {
  const rendered = await renderGuard([], true);
  assert.match(rendered, /退出 BalanceHub？/);
  assert.match(rendered, /有设置修改尚未保存/);
  assert.match(rendered, /这次修改会丢失/);
  assert.doesNotMatch(rendered, /运行中的任务/);

  const mixed = await renderGuard([task("checkin-1", "checkIn")], true);
  assert.match(mixed, /1 个任务仍在运行 · 有设置尚未保存/);
  assert.match(mixed, /这次修改会丢失/);
});

const hostNodeOps = {
  createElement: () => ({}),
  createText: () => ({}),
  createComment: () => ({}),
  setText: () => {},
  setElementText: () => {},
  insert: () => {},
  remove: () => {},
  parentNode: () => null,
  nextSibling: () => null,
  patchProp: () => {},
  cloneNode: (node: unknown) => node,
};
const headless = createRenderer<Record<string, never>, Record<string, never>>(
  hostNodeOps as never,
);

let guardComposableSource: string | undefined;
function loadGuardComposable(mocks: {
  listen: (event: string, callback: () => void) => Promise<() => void>;
  quit: () => void;
}) {
  guardComposableSource ??= ts.transpileModule(
    readFileSync(guardPath, "utf8"),
    { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } },
  ).outputText;
  const exports: { useAppExitGuard?: (options: unknown) => Record<string, any> } = {};
  const require = createRequire(import.meta.url);
  new Function("require", "exports", "module", guardComposableSource)(
    (specifier: string) => {
      if (specifier === "@arco-design/web-vue") return { Message: { error: () => {} } };
      if (specifier === "../api/transport.ts") {
        return { isWebHost: false, listen: mocks.listen };
      }
      if (specifier === "../api/app") {
        return {
          quitApp: async () => { mocks.quit(); },
          setAppRunningTasks: async () => {},
        };
      }
      if (specifier === "../utils/app-exit") return { exitBlockingTasks };
      return require(specifier);
    },
    exports,
    { exports },
  );
  assert.ok(exports.useAppExitGuard);
  return exports.useAppExitGuard;
}

function mountExitGuard(
  mocks: {
    listen: (event: string, callback: () => void) => Promise<() => void>;
    quit: () => void;
  },
  options: Record<string, unknown>,
) {
  const useAppExitGuard = loadGuardComposable(mocks);
  let guard!: Record<string, any>;
  const app = headless.createApp(defineComponent({
    setup() {
      guard = useAppExitGuard(options);
      return () => h("div");
    },
  }));
  app.mount({});
  return { guard, unmount: () => app.unmount() };
}

async function settle() {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

test("pending settings are flushed before a quit request turns into a dialog", async () => {
  let closeRequest: (() => void) | undefined;
  const calls = { quit: 0, flush: 0 };
  const pending = ref(true);
  const guardMount = mountExitGuard(
    {
      listen: async (_event, callback) => {
        closeRequest = callback;
        return () => {};
      },
      quit: () => { calls.quit += 1; },
    },
    {
      activeTasks: ref<BackgroundTask[]>([]),
      pendingSettings: pending,
      flushSettings: async () => {
        calls.flush += 1;
        pending.value = false;
        return true;
      },
    },
  );
  await settle();
  assert.ok(closeRequest);

  closeRequest!();
  await settle();

  assert.equal(calls.flush, 1);
  assert.equal(calls.quit, 1);
  assert.equal(guardMount.guard.exitConfirmVisible.value, false);
  guardMount.unmount();
});

test("a settings save that cannot land still explains itself before quitting", async () => {
  let closeRequest: (() => void) | undefined;
  const calls = { quit: 0, flush: 0 };
  const pending = ref(true);
  const guardMount = mountExitGuard(
    {
      listen: async (_event, callback) => {
        closeRequest = callback;
        return () => {};
      },
      quit: () => { calls.quit += 1; },
    },
    {
      activeTasks: ref<BackgroundTask[]>([]),
      pendingSettings: pending,
      flushSettings: async () => {
        calls.flush += 1;
        return false;
      },
    },
  );
  await settle();

  closeRequest!();
  await settle();

  assert.equal(calls.flush, 1);
  assert.equal(calls.quit, 0);
  assert.equal(guardMount.guard.exitConfirmVisible.value, true);
  assert.equal(guardMount.guard.exitConfirmSettingsPending.value, true);
  assert.deepEqual(guardMount.guard.exitConfirmTasks.value, []);
  guardMount.unmount();
});

test("cancelling while settings are still flushing prevents a late dialog or quit", async () => {
  let closeRequest: (() => void) | undefined;
  let release!: (value: boolean) => void;
  const calls = { quit: 0 };
  const pending = ref(true);
  const guardMount = mountExitGuard(
    {
      listen: async (_event, callback) => {
        closeRequest = callback;
        return () => {};
      },
      quit: () => { calls.quit += 1; },
    },
    {
      activeTasks: ref<BackgroundTask[]>([]),
      pendingSettings: pending,
      flushSettings: () => new Promise<boolean>((resolve) => { release = resolve; }),
    },
  );
  await settle();
  closeRequest!();
  await settle();
  // 用户关窗后改变主意：补写尚未返回就结束确认流程。
  guardMount.guard.cancelExit();
  pending.value = false;
  release(true);
  await settle();
  assert.equal(calls.quit, 0, "取消后旧请求不能再直接退出");
  assert.equal(guardMount.guard.exitConfirmVisible.value, false);
  guardMount.unmount();
});

test("running tasks still open the confirmation without touching settings", async () => {
  let closeRequest: (() => void) | undefined;
  const calls = { quit: 0, flush: 0 };
  const guardMount = mountExitGuard(
    {
      listen: async (_event, callback) => {
        closeRequest = callback;
        return () => {};
      },
      quit: () => { calls.quit += 1; },
    },
    {
      activeTasks: ref<BackgroundTask[]>([task("checkin-1", "checkIn")]),
      pendingSettings: ref(false),
      flushSettings: async () => {
        calls.flush += 1;
        return true;
      },
    },
  );
  await settle();

  closeRequest!();
  await settle();

  assert.equal(calls.flush, 0);
  assert.equal(calls.quit, 0);
  assert.equal(guardMount.guard.exitConfirmVisible.value, true);
  assert.equal(guardMount.guard.exitConfirmSettingsPending.value, false);
  assert.deepEqual(
    guardMount.guard.exitConfirmTasks.value.map((item: BackgroundTask) => item.id),
    ["checkin-1"],
  );
  guardMount.unmount();
});

test("nothing pending keeps the original immediate-quit behavior", async () => {
  let closeRequest: (() => void) | undefined;
  const calls = { quit: 0, flush: 0 };
  const guardMount = mountExitGuard(
    {
      listen: async (_event, callback) => {
        closeRequest = callback;
        return () => {};
      },
      quit: () => { calls.quit += 1; },
    },
    {
      activeTasks: ref<BackgroundTask[]>([]),
      pendingSettings: ref(false),
      flushSettings: async () => {
        calls.flush += 1;
        return true;
      },
    },
  );
  await settle();

  closeRequest!();
  await settle();

  assert.equal(calls.flush, 0);
  assert.equal(calls.quit, 1);
  guardMount.unmount();
});

test("desktop hosts route every quit path through the running-task guard", () => {
  const desktop = readFileSync(desktopPath, "utf8");
  const guardSource = readFileSync(guardPath, "utf8");

  assert.match(desktop, /fn request_app_exit\(/);
  assert.match(
    desktop,
    /if exit_guard_running\(&app_handle\) \{\s*api\.prevent_close\(\);\s*request_app_exit\(&app_handle\);/,
  );
  assert.match(desktop, /"quit" => request_app_exit\(app\)/);
  // macOS 原生 Quit 菜单项不会产生 ExitRequested，必须换成自定义项才能拦住 Cmd+Q。
  assert.match(desktop, /MenuItem::with_id\(\s*handle,\s*"app-quit"/);
  assert.match(desktop, /event\.id\(\)\.as_ref\(\) == "app-quit"/);
  assert.doesNotMatch(desktop, /quit_with_text/);
  assert.match(desktop, /RunEvent::ExitRequested/);
  assert.match(desktop, /api\.prevent_exit\(\)/);
  assert.match(desktop, /app-close-requested/);
  assert.match(guardSource, /listen\("app-close-requested"/);
  assert.match(guardSource, /setAppRunningTasks\(running\)/);
  assert.match(guardSource, /await quitApp\(\)/);
  // 弹窗打开后任务自行结束：列表跟着收缩，全部结束直接关闭，不能继续拦人。
  assert.match(guardSource, /if \(!exitConfirmVisible\.value\) return;/);
  assert.match(guardSource, /if \(tasks\.length === 0 && !pending\) \{\s*cancelExit\(\);/);
  // 防抖/失败中的设置同样要拦退出，并在弹窗前先尝试补写一次。
  assert.match(guardSource, /pendingSettings\?: Ref<boolean>/);
  assert.match(guardSource, /if \(settingsPending\.value && options\.flushSettings\)/);
  assert.match(guardSource, /await options\.flushSettings\(\)/);
  // 组件卸载后新实例必须重新同步，否则 Rust 侧保留 true 会永久拦住关闭。
  assert.match(guardSource, /reportedRunning = null;\s*void syncRunning\(false\);/);
});
