import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

function source(path: string) {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8");
}

function openingTags(markup: string, name: string) {
  return [...markup.matchAll(new RegExp(`<${name}\\b[\\s\\S]*?>`, "g"))].map((match) => match[0]);
}

test("总览弹窗全部改为首次打开时加载", () => {
  const overlays = source("../src/components/AppOverlays.vue");
  assert.doesNotMatch(
    overlays,
    /^import \w+Modal from "\.\/\w+Modal\.vue";/m,
    "弹窗不能再静态导入，否则打开任意一个都会加载全部窗口",
  );
  const declarations = [...overlays.matchAll(
    /const (\w+Modal) = defineAsyncComponent\(\(\) => import\("\.\/(\w+Modal)\.vue"\)\);/g,
  )];
  assert.ok(declarations.length >= 13, "每个弹窗都要有对应的异步组件声明");
  assert.match(overlays, /useFirstOpenLatch\(/, "异步弹窗必须由首次打开闸门控制挂载");

  for (const [, name, file] of declarations) {
    const tags = openingTags(overlays, name);
    assert.ok(tags.length > 0, `${name} 没有在模板中使用`);
    for (const tag of tags) {
      // BatchOperationProgressModal 同一组件有两处入口，两处都必须带闸门。
      assert.match(tag, /v-if="\w+Opened"/, `${name} 的模板入口缺少首次打开闸门`);
    }
    assert.equal(name.replace(/Modal$/, ""), file.replace(/Modal$/, ""));
  }
});

test("Agent 工作台的窗口随首次打开加载", () => {
  const dashboard = source("../src/components/agent-workspace/AgentDashboard.vue");
  const modals = [
    "AgentConfigurationEditorModal",
    "AgentConfigurationPreviewModal",
    "AgentCatalogDetailDrawer",
    "AgentCatalogDefinitionForm",
    "AgentCatalogDefinitionModal",
    "AgentCatalogPlanModal",
    "AgentCatalogRelationModal",
    "AgentInstallationModal",
    "AgentLaunchProviderModal",
    "AgentAssetDetailDrawer",
    "AgentAssetPlanModal",
    "AgentAssetAccessModal",
    "AgentHookPlanModal",
    "AgentSelfCheckModal",
    "AgentHistoryDrawer",
    "AgentMaintenancePlanModal",
  ];
  for (const name of modals) {
    assert.doesNotMatch(
      dashboard,
      new RegExp(`^import ${name} from `, "m"),
      `${name} 不能静态导入工作台，否则打开 Agent 页就带上编辑器等重量级依赖`,
    );
    assert.match(
      dashboard,
      new RegExp(`const ${name} = defineAsyncComponent\\(`),
      `${name} 应改为按需加载`,
    );
    assert.match(
      dashboard,
      new RegExp(`<${name}\\s+v-if="\\w+Opened`),
      `${name} 的模板入口缺少首次打开闸门`,
    );
  }
  // 定义编辑器带 CodeMirror：工作台静态图里不能再出现编辑器块。
  assert.doesNotMatch(dashboard, /^import ContentEditor from |^import CodeEditor from /m);
});

test("Agent 工作台按页面加载，首次进入后再保持挂载", () => {
  const dashboard = source("../src/components/agent-workspace/AgentDashboard.vue");
  for (const name of [
    "AgentCatalogPanel",
    "AgentSessionPanel",
    "AgentInstructionsPanel",
    "AgentWslPanel",
    "ProviderRecommendationModal",
  ]) {
    assert.match(
      dashboard,
      new RegExp(
        `const ${name} = defineAsyncComponent\\(\\(\\) => import\\("\\.\\.?/[^"]*${name}\\.vue"\\)\\);`,
      ),
      `${name} 应改为按需加载`,
    );
    for (const tag of openingTags(dashboard, name)) {
      assert.match(
        tag,
        /v-(?:else-)?if="[^"]*Opened(?:[^"]*)?|v-if="wsl\.supported/,
        `${name} 的模板入口缺少首次打开闸门`,
      );
    }
  }
  assert.match(dashboard, /const catalogOpened = useFirstOpenLatch\(/);
  assert.match(dashboard, /const recommendationOpened = useFirstOpenLatch\(/);
});

test("设置分区按当前分区加载", () => {
  const drawer = source("../src/components/SettingsDrawer.vue");
  for (const name of [
    "About",
    "Accounts",
    "Appearance",
    "Automation",
    "Data",
    "Network",
    "Notification",
    "Terminal",
  ]) {
    const component = `Settings${name}Section`;
    assert.doesNotMatch(
      drawer,
      new RegExp(`^import ${component} from `, "m"),
      `${component} 不能静态导入设置抽屉，否则打开设置就下载全部分区`,
    );
    assert.match(
      drawer,
      new RegExp(`const ${component} = defineAsyncComponent\\(`),
      `${component} 应改为按需加载`,
    );
    assert.match(
      drawer,
      new RegExp(`<${component}\\s+v-(?:else-)?if="[^"]*sectionLoaded\\('`),
      `${component} 的模板入口缺少分区加载闸门`,
    );
  }
  assert.match(drawer, /const loadedSections = ref<ReadonlySet<string>>\(new Set\(\)\)/);
});

test("设置分区与 CPA 面板不再静态带入编辑器", () => {
  // 这几个面板都会在首屏之外打开，但静态导入 ContentEditor 会把 565KB 的
  // CodeMirror 一起拖进来，只有真正编辑时才该下载。
  for (const file of [
    "../src/components/oauth/CpaGatewayPanel.vue",
    "../src/components/oauth/CpaSchedulingSection.vue",
    "../src/components/oauth/CpaModelRulesSection.vue",
  ]) {
    const markup = source(file);
    assert.doesNotMatch(
      markup,
      /^import (ContentEditor|CodeEditor) from /m,
      `${file} 不能静态导入编辑器`,
    );
  }
  const scheduling = source("../src/components/oauth/CpaSchedulingSection.vue");
  assert.match(scheduling, /const CpaConfigDialog = defineAsyncComponent\(/);
  assert.match(scheduling, /v-if="configOpened"/);
  const rules = source("../src/components/oauth/CpaModelRulesSection.vue");
  assert.match(rules, /const ContentEditor = defineAsyncComponent\(/);
  assert.match(rules, /v-if="aliasEditorOpened"/);
  const gateway = source("../src/components/oauth/CpaGatewayPanel.vue");
  assert.match(gateway, /const CpaPluginsDialog = defineAsyncComponent\(/);
  assert.match(gateway, /v-if="pluginsOpened"/);
});

test("按需界面专属样式随界面加载，不进入首屏 CSS", () => {
  const app = source("../src/styles/app.css");
  for (const [file, component] of [
    ["request-logs", "RequestLogsModal"],
    ["capability-probe", "CapabilityProbeModal"],
    ["batch-operation", "BatchOperationProgressModal"],
    ["provider-detail", "ProviderDetailModal"],
    ["cli-config-key-picker", "CliConfigKeyPickerModal"],
    ["site-announcements", "SiteAnnouncementsModal"],
    ["checkin-calendar", "CheckInCalendarModal"],
    ["liveness-details", "LivenessDetailsModal"],
    ["app-exit-confirm", "AppExitConfirmModal"],
    ["cli-session-detail", "agent-workspace/SessionReader"],
    ["usage", "UsageTrendModal"],
    ["usage", "agent-workspace/SessionUsage"],
    ["usage", "agent-workspace/UsageReconciliationModal"],
    ["api-key-manager", "provider-editor/ProviderApiKeyVault"],
    ["api-key-manager", "provider-editor/ProviderApiKeyEditorForm"],
    ["agent-environment", "agent-workspace/AgentDashboard"],
    ["agent-workspace", "agent-workspace/AgentDashboard"],
    ["agent-sessions", "agent-workspace/AgentDashboard"],
    ["agent-sessions", "WorkspaceSessionHistoryPanel"],
    ["agent-runtime", "AgentRuntimeList"],
    ["available-models", "AvailableModelsModal"],
    ["app-update", "AppUpdateModal"],
    ["onboarding", "AppOnboardingModal"],
    ["workspace-picker", "WorkspacePickerModal"],
    ["temporary-cli-preview", "TemporaryCliLaunchPreviewModal"],
    ["surfaces/nav-rail", "SettingsDrawer"],
    ["surfaces/nav-rail", "provider-editor/ProviderEditorNav"],
    ["surfaces/provider-auth", "provider-editor/ProviderEditorCredentialsSection"],
  ]) {
    assert.doesNotMatch(
      app,
      new RegExp(`modules/${file}\\.css`),
      `${file}.css 只服务于按需界面，不能继续放在 app.css 里`,
    );
    assert.match(
      source(`../src/components/${component}.vue`),
      new RegExp(`styles/modules/${file}\\.css`),
      `${component}.vue 没有引入 ${file}.css`,
    );
  }
});

test("Agent 工作台的错误提示样式不拖累 CPA 编辑器", () => {
  const editor = source("../src/components/ContentEditor.vue");
  // agent-workspace.css 现在只随工作台加载；编辑器自己的提示必须在本地收口。
  assert.doesNotMatch(editor, /class="agent-workspace-error"/);
  assert.match(editor, /class="content-editor-notice"/);
  assert.match(editor, /<style scoped>/);
  assert.match(editor, /\.content-editor-notice \{/);
});
