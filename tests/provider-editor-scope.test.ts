import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  providerEditorTitle,
  providerScopeNoun,
} from "../src/utils/provider-protocol.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("the editor names what it is editing", () => {
  assert.equal(providerScopeNoun("newApi"), "中转站");
  assert.equal(providerScopeNoun("sub2Api"), "中转站");
  assert.equal(providerScopeNoun("api"), "中转站");
  assert.equal(providerScopeNoun("oauth"), "账号");
  assert.equal(providerScopeNoun("cliProxyApi"), "网关");

  // 标题跟着选中的协议走，窗口不再固定叫「中转站」。
  assert.equal(providerEditorTitle(false, "newApi"), "添加中转站");
  assert.equal(providerEditorTitle(true, "newApi"), "编辑中转站");
  assert.equal(providerEditorTitle(false, "cliProxyApi"), "添加网关");
  assert.equal(providerEditorTitle(true, "cliProxyApi"), "编辑网关");
  assert.equal(providerEditorTitle(true, "oauth"), "编辑账号");
});

test("the editor's labels, save button and picker share the same noun", () => {
  const basics = source("../src/components/provider-editor/ProviderEditorBasicsSection.vue");
  const automation = source("../src/components/provider-editor/ProviderEditorAutomationSection.vue");
  const drawer = source("../src/components/ProviderEditorDrawer.vue");
  const state = source("../src/composables/useProviderEditorState.ts");
  const styles = source("../src/styles/modules/surfaces/dialogs.css");

  // 类型不再是中转站专属的说法：这里同时可以选到 CPA 网关。
  assert.doesNotMatch(basics, /label="中转站类型"/);
  assert.match(basics, /label="接入类型"/);
  // 地址字段和占位示例跟着称呼走：CPA 给出本机默认端口。
  assert.match(basics, /:label="`\$\{scopeNoun\}地址`"/);
  assert.match(basics, /http:\/\/127\.0\.0\.1:8317/);
  // 每种接入的说明来自协议目录，直接显示在选型卡里，不再只做悬停提示。
  assert.match(basics, /description: descriptor\.description/);
  assert.match(basics, /provider-protocol-option-description/);

  // 保存按钮和标题读同一份称呼，草稿里换了协议就一起改口。
  assert.match(drawer, /保存\$\{scopeNoun\}/);
  assert.doesNotMatch(drawer, /保存中转站/);
  assert.match(
    state,
    /providerEditorTitle\(Boolean\(editingProviderId\.value\), draftProvider\.identity\.protocol\)/,
  );

  // 签到开关跟着协议能力出现：网关和授权账号没有账号签到接口。
  assert.match(automation, /\.capabilities\.checkIn \?\? false/);
  assert.match(automation, /<ProviderEditorCheckInSection v-if="checkInSupported"/);

  // 四个协议各带一句说明后，2×2 比三段式一排更好读。
  assert.match(
    styles,
    /\.provider-protocol-picker \{[^}]*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\)/s,
  );
  assert.match(styles, /\.provider-protocol-option-description \{[^}]*font-size: 11px/s);
});

test("the editor opens one navigable section at a time", () => {
  const drawer = source("../src/components/ProviderEditorDrawer.vue");
  const nav = source("../src/components/provider-editor/ProviderEditorNav.vue");
  const sections = source("../src/composables/useProviderEditorSections.ts");

  // 长表单拆成左侧分区、右侧单页；深链用分区状态落位，不再靠 offsetTop 猜位置。
  assert.match(drawer, /<ProviderEditorNav/);
  assert.match(drawer, /v-else-if="activeSection === 'credentials'"/);
  assert.doesNotMatch(drawer, /运行策略/);
  assert.doesNotMatch(drawer, /scrollToInitialSection|offsetTop/);
  assert.match(nav, /aria-label="编辑分区"/);
  assert.match(nav, /surface-nav-item/);

  // 分区按协议能力出现：余额接口只给声明了自定义接口的协议，请求配置在
  // 备用地址、请求头、Key 发送方式都不可用时整体隐藏。
  assert.match(sections, /capabilities\.value\?\.quotaSpec/);
  assert.match(sections, /requestSupported/);
  assert.match(sections, /capabilities\.value\?\.maxBackupUrls/);
});
