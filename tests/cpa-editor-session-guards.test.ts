import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("CPA editors confirm before dropping dirty drafts", () => {
  const config = source("../src/components/oauth/CpaConfigDialog.vue");
  const plugins = source("../src/components/oauth/CpaPluginsDialog.vue");
  const rules = source("../src/components/oauth/CpaModelRulesSection.vue");
  const fields = source("../src/components/oauth/CpaGatewayPanel.vue");

  for (const [name, text] of Object.entries({ config, plugins, rules, fields })) {
    assert.match(text, /confirmDiscardChanges/, `${name} 应复用放弃修改确认`);
  }
  assert.match(config, /async function requestClose\(\)/);
  assert.match(config, /@cancel="requestClose"/);
  assert.match(plugins, /async function requestCloseConfig\(\)/);
  assert.match(plugins, /@click="requestCloseConfig"/);
  assert.match(plugins, /@cancel="requestClose"/);
  assert.match(rules, /async function requestCloseEditor\(\)/);
  assert.match(rules, /@cancel="requestCloseEditor"/);
  assert.match(fields, /async function requestCloseFieldsEditor\(\)/);
  assert.match(fields, /@cancel="requestCloseFieldsEditor"/);
  assert.match(fields, /@click="requestCloseFieldsEditor"/);
});

test("a late CPA save cannot write into a freshly opened editor", () => {
  const config = source("../src/components/oauth/CpaConfigDialog.vue");
  const plugins = source("../src/components/oauth/CpaPluginsDialog.vue");
  const rules = source("../src/components/oauth/CpaModelRulesSection.vue");
  const fields = source("../src/components/oauth/CpaGatewayPanel.vue");
  const keys = source("../src/components/oauth/CpaClientKeysSection.vue");
  const credential = source("../src/components/oauth/OAuthCredentialEditor.vue");

  // 每次保存捕获打开会话，返回后只有会话仍然一致才写回或关闭。
  assert.match(config, /const session = sequence;\s*const token = \+\+saveSequence;/);
  assert.match(config, /session !== sequence \|\| !props\.visible/);
  assert.match(config, /if \(token === saveSequence\) saving\.value = false/);
  assert.match(plugins, /const session = configSession;/);
  assert.match(plugins, /session !== configSession \|\| editing\.value\?\.id !== plugin\.id/);
  assert.match(plugins, /function closeConfig\(\) \{[\s\S]*?configSaving\.value = false;/);
  assert.match(rules, /const session = editorSession;/);
  assert.match(rules, /session !== editorSession/);
  assert.match(rules, /if \(token === saveSequence\) rules\.value = result/);
  assert.match(rules, /if \(token === saveSequence\) saving\.value = false/);
  assert.match(fields, /const session = editorSession;/);
  assert.match(fields, /if \(session === editorSession\) closeFieldsEditor\(\)/);
  assert.match(credential, /const session = request;/);
  assert.match(credential, /session !== request \|\|/);
  assert.match(credential, /if \(session === request && props\.visible\)/);
  // 关闭授权原文编辑器同样要确认，避免整份令牌文本被误关丢弃。
  assert.match(credential, /async function requestClose\(\)/);
  assert.match(credential, /@close="requestClose"/);
  assert.match(credential, /@click="requestClose"/);
  // 钥匙列表的写操作跟随站点/凭据版本失效，后台刷新不能把旧结果写进新视图。
  assert.match(keys, /const view = keysView;/);
  assert.match(keys, /view === keysView/);
  assert.match(keys, /keysSaving\.value = false;/);
});

test("delete confirmation names the credentials and tasks it destroys", () => {
  const actions = source("../src/composables/useProviderActions.ts");
  assert.match(actions, /凭据和临时 CLI 偏好会一并删除/);
  assert.match(actions, /未完成的签到和登录任务会被取消/);
});

test("about-page directory and repository buttons surface opener failures", () => {
  const about = source("../src/components/settings/SettingsAboutSection.vue");
  assert.match(about, /async function runOpener/);
  assert.match(about, /withTimeout\(action\(\), 15_000/);
  assert.match(about, /Message\.error\(`\$\{failure\}/);
  assert.doesNotMatch(about, /void open(ProjectRepository|LogDirectory|DataDirectory)\(\)/);
});

test("OAuth login tasks are deduplicated per target provider, not just vendor", () => {
  const login = source("../crates/core/src/services/oauth_accounts/login.rs");
  assert.match(login, /fn same_login_target\(/);
  assert.match(login, /task\.provider_id\.as_deref\(\) == provider_id/);
  assert.match(login, /same_login_target\(&s\.task, vendor, target_id\)/);
});
