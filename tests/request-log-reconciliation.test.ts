import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

function source(relativePath: string) {
  return readFileSync(
    fileURLToPath(new URL(`../${relativePath}`, import.meta.url)),
    "utf8",
  );
}

test("请求日志按错误率、耗时、模型和 Key 给出可读聚合", () => {
  const modal = source("src/components/RequestLogsModal.vue");

  assert.match(modal, /本页错误率/);
  assert.match(modal, /logStatusTone\(log\) === "error"/);
  assert.match(modal, /本页平均耗时/);
  assert.match(modal, /label: "主要模型"/);
  assert.match(modal, /label: "主要 Key"/);
  // 聚合只代表当前页，界面必须说明，避免读成整个时间范围。
  assert.match(modal, /只统计当前页/);
});

test("请求日志可以把当前上下文交给本地会话对账", () => {
  const modal = source("src/components/RequestLogsModal.vue");
  const overlays = source("src/components/AppOverlays.vue");
  const app = source("src/App.vue");
  const controller = source("src/composables/useAppController.ts");
  const state = source("src/composables/useUsageReconciliation.ts");

  assert.match(modal, /reconcile: \[\];/);
  assert.match(modal, /emit\('reconcile'\)/);
  assert.match(modal, /本地会话对账/);
  // 先收起请求日志再打开对账，避免两个大弹窗叠在一起。
  assert.match(
    overlays,
    /@reconcile="requestLogsVisible = false; emit\('openUsageReconciliation'\)"/,
  );
  assert.match(app, /@open-usage-reconciliation="app\.openUsageReconciliation"/);
  assert.match(app, /<UsageReconciliationModal/);
  assert.match(app, /:visible="app\.usageReconciliationVisible"/);
  assert.match(controller, /const usageReconciliation = useUsageReconciliation\(\)/);
  assert.match(controller, /\.\.\.usageReconciliation/);
  assert.match(state, /usageReconciliationVisible: visible/);
  assert.match(state, /function openUsageReconciliation\(\)/);
});
