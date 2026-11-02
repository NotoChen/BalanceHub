import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { toggleExactModelRule } from "../src/utils/model-rule-input.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("picking a model writes one exact rule and picking it again removes it", () => {
  assert.deepEqual(toggleExactModelRule([], "deepseek-v4-flash"), ["deepseek-v4-flash"]);
  assert.deepEqual(
    toggleExactModelRule(["claude-*", "gpt-5.5"], "gpt-5.5"),
    ["claude-*"],
  );
  assert.deepEqual(
    toggleExactModelRule(["claude-*"], "gpt-5.5"),
    ["claude-*", "gpt-5.5"],
  );
  // 空白输入不写入规则，避免产生一条永远匹配不到的条目。
  assert.deepEqual(toggleExactModelRule(["gpt-5.5"], "  "), ["gpt-5.5"]);
});

test("model rules can be picked from the site's model list", () => {
  const section = source("../src/components/provider-editor/ProviderEditorModelConfigSection.vue");
  const picker = source("../src/components/provider-editor/ProviderModelRuleEditor.vue");

  assert.match(section, /<ProviderModelRuleEditor\s+v-model="draft\.modelConfig\.include"/s);
  assert.match(section, /<ProviderModelRuleEditor\s+v-model="draft\.modelConfig\.exclude"/s);
  assert.match(picker, /toggleExactModelRule/);
  assert.match(picker, /从站点模型选择/);
  assert.match(picker, /勾选写入精确模型 ID/);
});

test("rare editor knobs stay behind disclosures", () => {
  const automation = source("../src/components/provider-editor/ProviderEditorAutomationSection.vue");
  const checkIn = source("../src/components/provider-editor/ProviderEditorCheckInSection.vue");

  // 刷新间隔、测活方式、签到方式和时间仍然直接可见。
  assert.match(automation, /label="刷新间隔"/);
  assert.match(automation, /label="自动测活"/);
  assert.match(checkIn, /label="签到方式"/);
  assert.match(checkIn, /label="签到时间"/);

  // 话术、按 Agent 指定地址、站点防护与 Turnstile 收进折叠区。
  assert.match(automation, /<summary>更多测活设置<\/summary>/);
  assert.doesNotMatch(automation, /<summary>按 Agent 指定地址（可选）<\/summary>/);
  assert.match(checkIn, /<summary>更多签到设置<\/summary>/);
});

test("the quota section reads expressions in place", () => {
  const quota = source("../src/components/provider-editor/ProviderEditorQuotaSpecSection.vue");
  const drawer = source("../src/components/ProviderEditorDrawer.vue");

  // 表达式写完后直接在分区里读取一次，结果贴在字段上方。
  assert.match(quota, /@click="emit\('test-connection'\)"/);
  assert.match(quota, /provider-quota-preview/);
  assert.match(quota, /<summary>鉴权与请求头（可选）<\/summary>/);
  assert.match(drawer, /:connection-test-result="connectionTestResult"/);
  // 订阅窗口上限来自协议能力，界面不再自己写死一个数字。
  assert.match(quota, /maxQuotaWindows/);
  assert.doesNotMatch(quota, /windows\.length >= 3/);
});

test("the endpoint catalog shows every protocol request and keeps overrides scoped", () => {
  const section = source("../src/components/provider-editor/ProviderEditorRequestSection.vue");
  const catalog = source("../src/components/provider-editor/ProviderEndpointCatalog.vue");
  const input = source("../src/utils/provider-input.ts");

  // 只有后端给出请求目录时才出现，数量上限同样来自协议能力。
  assert.match(section, /endpointLimit > 0 && endpoints\.length/);
  assert.match(section, /:limit="endpointLimit"/);
  assert.match(section, /:endpoints="endpoints"/);

  // 默认请求、认证方式和请求头必须原样列出。
  assert.match(catalog, /endpoint\.method/);
  assert.match(catalog, /endpoint\.path/);
  assert.match(catalog, /endpoint\.authLabel/);
  assert.match(catalog, /endpoint\.headers/);
  assert.match(catalog, /endpoint\.placeholders/);

  // 改写只维护路径、查询参数和该接口的请求头。
  assert.match(catalog, /overrideOf\(endpoint\.id\)/);
  assert.match(catalog, /addQuery\(endpoint\.id\)/);
  assert.match(catalog, /addHeader\(endpoint\.id\)/);
  assert.match(catalog, /留空删除同名默认参数/);
  assert.match(catalog, /恢复默认/);
  assert.match(catalog, /保留 <code>\{code\}<\/code> 占位符/);

  // 新草稿和保存映射都带上接口覆盖，避免再次打开时丢字段。
  assert.match(input, /endpoints: \[\]/);
  assert.match(input, /endpoints: \(provider\.request\?\.endpoints/);
});

test("the active credential mode is distinguishable from the other ones", () => {
  const credentials = source("../src/components/provider-editor/ProviderEditorCredentialsSection.vue");
  assert.match(credentials, /'is-active': draft\.auth\.mode === mode\.mode/);
});
