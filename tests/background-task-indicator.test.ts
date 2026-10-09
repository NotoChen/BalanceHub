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
import * as progressDisplay from "../src/utils/progress-display.ts";

const componentPath = fileURLToPath(
  new URL("../src/components/BackgroundTaskIndicator.vue", import.meta.url),
);
const stylePath = fileURLToPath(
  new URL("../src/styles/modules/topbar.css", import.meta.url),
);

test("background task entry keeps its identity icon and uses color flow for activity", () => {
  const component = readFileSync(componentPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  assert.match(component, /useId/);
  assert.doesNotMatch(component, /LoaderCircle/);
  assert.doesNotMatch(component, /topbar-action-spin/);
  assert.match(styles, /@keyframes background-task-gradient-flow/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
});

let indicator: Component | undefined;
function loadIndicator(): Component {
  if (indicator) return indicator;
  const { descriptor } = parse(readFileSync(componentPath, "utf8"), { filename: componentPath });
  const compiled = compileScript(descriptor, { id: "background-task-result-test", inlineTemplate: true });
  const output = ts.transpileModule(compiled.content, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports: { default?: Component } = {};
  const require = createRequire(import.meta.url);
  new Function("require", "exports", output)((specifier: string) => {
    if (specifier === "../utils/progress-display") return progressDisplay;
    return require(specifier);
  }, exports);
  assert.ok(exports.default);
  indicator = exports.default;
  return indicator;
}

function task(id: string, status: BackgroundTask["status"]): BackgroundTask {
  return { id, kind: "cliLaunch", title: id, detail: "隔离任务", status, progress: null, startedAt: 1, source: "manual" };
}

async function renderIndicator(tasks: BackgroundTask[], recentTasks: BackgroundTask[] = [], onClearRecent?: () => void) {
  const app = createSSRApp(loadIndicator(), { tasks, recentTasks, activeCount: tasks.length, onClearRecent });
  app.component("a-popover", defineComponent({
    emits: ["update:popupVisible", "popupVisibleChange"],
    setup(_props, { slots, emit }) {
      emit("update:popupVisible", false);
      emit("popupVisibleChange", false);
      return () => h("div", [slots.default?.(), slots.content?.()]);
    },
  }));
  app.component("a-progress", defineComponent({ render: () => h("div") }));
  return renderToString(app);
}

test("completed login results render their account and credential destinations", async () => {
  const recentTasks = ["查看登录账号", "查看站点凭据"].map((label, index) => ({
    ...task(`completed-${index}`, "success"), kind: "providerLogin" as const, title: "登录已完成", detail: "结果已保存", finishedAt: 2,
    actions: [{ label, run: () => {} }],
  }));
  const rendered = await renderIndicator([], recentTasks);
  assert.match(rendered, /<button\b[^>]*>查看登录账号<\/button>/);
  assert.match(rendered, /<button\b[^>]*>查看站点凭据<\/button>/);
  assert.doesNotMatch(rendered, /显示登录窗口/);
});

test("closing the task popup retains all recent results until the user clears them", async () => {
  const recentTasks = Array.from({ length: 12 }, (_, index) => ({ ...task(`recent-result-${index}`, "success"), finishedAt: 2 }));
  let cleared = 0;
  const rendered = await renderIndicator([], recentTasks, () => { cleared++; });
  for (const item of recentTasks) assert.ok(rendered.includes(item.title));
  assert.equal(cleared, 0);
  assert.match(rendered, /清空记录/);
});

test("running task progress renders at most two decimal places", async () => {
  const rendered = await renderIndicator([{ ...task("download", "running"), progress: 0.123456789 }]);
  assert.match(rendered, /12\.35%/);
  assert.doesNotMatch(rendered, /12\.3456789/);
});
