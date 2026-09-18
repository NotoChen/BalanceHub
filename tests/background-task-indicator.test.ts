import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { createSSRApp, defineComponent, h, type Component } from "vue";
import { compileScript, parse } from "vue/compiler-sfc";
import { renderToString } from "vue/server-renderer";
import ts from "typescript";

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
  assert.match(component, /:color="activeCount > 0 \? `url\(#\$\{gradientId\}\)`/);
  assert.doesNotMatch(component, /LoaderCircle/);
  assert.doesNotMatch(component, /topbar-action-spin/);
  assert.match(styles, /@keyframes background-task-gradient-flow/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
});

test("completed login results render their account and credential destinations", async () => {
  const { descriptor } = parse(readFileSync(componentPath, "utf8"), { filename: componentPath });
  const compiled = compileScript(descriptor, { id: "background-task-result-test", inlineTemplate: true });
  const output = ts.transpileModule(compiled.content, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports: { default?: Component } = {};
  new Function("require", "exports", output)(createRequire(import.meta.url), exports);
  assert.ok(exports.default);
  const recentTasks = ["查看登录账号", "查看站点凭据"].map((label, index) => ({
    id: `completed-${index}`, kind: "providerLogin", title: "登录已完成", detail: "结果已保存",
    status: "success", progress: null, startedAt: 1, finishedAt: 2, source: "manual",
    actions: [{ label, run: () => {} }],
  }));
  const app = createSSRApp(exports.default, { tasks: [], recentTasks, activeCount: 0 });
  app.component("a-popover", defineComponent({
    setup(_props, { slots }) { return () => h("div", [slots.default?.(), slots.content?.()]); },
  }));
  app.component("a-progress", defineComponent({ render: () => h("div") }));
  const rendered = await renderToString(app);
  assert.match(rendered, /<button\b[^>]*>查看登录账号<\/button>/);
  assert.match(rendered, /<button\b[^>]*>查看站点凭据<\/button>/);
  assert.doesNotMatch(rendered, /显示登录窗口/);
});
