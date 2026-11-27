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

test("点击中转站卡片打开详情，而不是直接进入编辑或签到", () => {
  const controller = source("src/composables/useProviderWorkspaceController.ts");
  const clickHandler = controller.slice(
    controller.indexOf("async function handleProviderCardClick"),
    controller.indexOf("return {", controller.indexOf("async function handleProviderCardClick")),
  );

  assert.match(controller, /openDetail: \(provider: Provider\) => void;/);
  assert.match(clickHandler, /options\.openDetail\(provider\);/);
  assert.doesNotMatch(clickHandler, /checkIn/);
  assert.doesNotMatch(clickHandler, /openEditProvider/);
  // OAuth / CPA 卡仍交给账号面板：那里已经有授权、额度和测活的完整事实。
  assert.match(clickHandler, /oauth\.openAccount\(provider\.identity\.id, provider\.actions\.cpaGateway\)/);
});

test("详情面按 id 跟随 store，刷新后不丢当前卡片", () => {
  const detail = source("src/composables/useProviderDetail.ts");

  assert.match(detail, /providers\.value\.find\(\(item\) => item\.identity\.id === providerId\.value\)/);
  assert.match(detail, /function openProviderDetail\(target: Provider\)/);
  assert.match(detail, /providerId\.value = target\.identity\.id;/);
  assert.match(detail, /providerDetailVisible: visible/);
});

test("详情面收口状态、绑定、能力台账和自动化事实", () => {
  const modal = source("src/components/ProviderDetailModal.vue");

  // 额度窗口复用 OAuth 卡片同一支条，不另画一套进度条。
  assert.match(modal, /import AccountQuotaBar from "\.\/oauth\/AccountQuotaBar\.vue"/);
  assert.match(modal, /providerQuotaBarWindow\(row\)/);
  // 能力名称只保留一份，详情与能力探测弹窗读同一个词表。
  assert.match(modal, /providerOptionalCapabilityLabel\(state\.capability\)/);
  assert.match(modal, /providerCardStatusLabel\(/);
  // 签到是按钮，不是点击卡片的副作用。
  assert.match(modal, /emit\('checkIn'/);
  assert.match(modal, /今天已签到/);
  assert.match(modal, /立即签到/);
  // 绑定行同时读本机 CLI 配置和 per-Agent 模型绑定。
  assert.match(modal, /store\.cliRuntime\.configs/);
  assert.match(modal, /provider\.cli\.agentModels/);
  assert.match(modal, /provider\.cli\.preferredModel/);
});

test("App 挂载详情面并把动作接回现有控制器", () => {
  const app = source("src/App.vue");

  assert.match(
    app,
    /const ProviderDetailModal = defineAsyncComponent\(\s*\(\) => import\("\.\/components\/ProviderDetailModal\.vue"\)\s*\)/,
  );
  assert.match(app, /<ProviderDetailModal\s+v-if="providerDetailOpened"/);
  assert.match(app, /<ProviderDetailModal/);
  assert.match(app, /v-model:visible="app\.providerDetailVisible"/);
  assert.match(app, /:provider="app\.providerDetailProvider"/);
  assert.match(app, /@edit="app\.editProvider"/);
  assert.match(app, /@check-in="app\.checkInProvider"/);
  assert.match(app, /@switch-cli-config="app\.switchProviderCliConfig"/);
  assert.match(app, /@launch-temporary-cli="app\.launchTemporaryCli"/);
});

test("详情覆盖从应用控制器导出，卡片点击接到同一份实例", () => {
  const controller = source("src/composables/useAppController.ts");

  assert.match(controller, /import \{ useProviderDetail \} from "\.\/useProviderDetail"/);
  assert.match(controller, /const providerDetail = useProviderDetail\(providers\)/);
  assert.match(
    controller,
    /openDetail: \(provider\) => providerDetail\.openProviderDetail\(provider\)/,
  );
  assert.match(controller, /\.\.\.providerDetail/);
  assert.match(controller, /\n    settings,\n/);
});

test("详情样式随详情弹窗加载", () => {
  const styles = source("src/styles/app.css");
  assert.doesNotMatch(styles, /@import "\.\/modules\/provider-detail\.css";/);
  assert.match(
    source("src/components/ProviderDetailModal.vue"),
    /styles\/modules\/provider-detail\.css/,
  );
});

test("能力探测弹窗与详情共用同一份能力名称", () => {
  const probe = source("src/components/CapabilityProbeModal.vue");

  assert.match(
    probe,
    /import \{\s*providerCapabilitySourceLabel,\s*providerOptionalCapabilityLabel,?\s*\} from "\.\.\/utils\/provider-capabilities"/,
  );
  assert.match(probe, /providerOptionalCapabilityLabel\("pricing"\)/);
  assert.match(probe, /providerOptionalCapabilityLabel\("models"\)/);
  assert.doesNotMatch(probe, /label: "站点定价"/);
});
