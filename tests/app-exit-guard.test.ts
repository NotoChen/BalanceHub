import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { createSSRApp, defineComponent, h, type Component } from "vue";
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
    task("manual-batch-operation", "refresh"),
    task("checkin-1", "checkIn"),
    task("temporary-cli-launch-1", "cliLaunch"),
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
  assert.equal(exitTaskSummary(tasks), "7 个任务仍在运行");
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

function renderGuard(tasks: BackgroundTask[]) {
  const app = createSSRApp(loadGuard(), { visible: true, tasks });
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
  assert.match(guardSource, /if \(tasks\.length === 0\) \{\s*cancelExit\(\);/);
  // 组件卸载后新实例必须重新同步，否则 Rust 侧保留 true 会永久拦住关闭。
  assert.match(guardSource, /reportedRunning = null;\s*void syncRunning\(false\);/);
});
