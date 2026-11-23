import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("closing the provider editor asks before dropping unsaved edits", () => {
  const drawer = source("../src/components/ProviderEditorDrawer.vue");
  const state = source("../src/composables/useProviderEditorState.ts");
  const drawers = source("../src/components/AppDrawers.vue");
  const app = source("../src/App.vue");

  // 表单很大，误点遮罩/关闭按钮不应静默丢掉这次输入。
  assert.match(drawer, /const hasUnsavedChanges = computed/);
  assert.match(drawer, /async function requestClose\(\)/);
  assert.match(drawer, /await confirmDiscardChanges\(\)/);
  assert.match(drawer, /@click="requestClose"/);
  assert.match(
    drawer,
    /@update:visible="\(value\) => value \? emit\('update:visible', true\) : requestClose\(\)"/,
  );
  // 保存中、登录启动中和凭据助手执行中都有各自的收口逻辑，不再叠加确认。
  assert.match(drawer, /props\.saving[\s\S]*props\.startingBrowserLogin[\s\S]*props\.credentialAssistantBusy[\s\S]*!hasUnsavedChanges\.value/);

  // 后台 Key 轮换/令牌续期会改草稿，但那不是用户编辑：同步后要重设基线。
  assert.match(state, /const editorDraftRevision = ref\(0\)/);
  assert.match(
    state,
    /availableModels\.value = \[\.\.\.provider\.capabilities\.availableModels\];\s*editorDraftRevision\.value \+= 1;/,
  );
  assert.match(drawers, /:draft-revision="providerEditorDraftRevision"/);
  assert.match(app, /:provider-editor-draft-revision="app\.editorDraftRevision"/);

  // 同步前的草稿快照一路传到窗口：只把后台真正改过的字段并入基线，
  // 用户在同一窗口里改过的其它字段仍然触发关闭确认。
  assert.match(
    state,
    /function syncManagedApiKeys\(provider: Provider\) \{\s*if \(editingProviderId\.value !== provider\.identity\.id\) return;\s*editorDraftBeforeSync\.value = JSON\.stringify\(draftProvider\);/,
  );
  assert.match(drawer, /adoptSyncedDraftFields/);
  assert.match(drawer, /props\.draftBeforeSync/);
  assert.match(drawers, /:draft-before-sync="providerEditorDraftBeforeSync"/);
  assert.match(
    app,
    /:provider-editor-draft-before-sync="app\.editorDraftBeforeSync"/,
  );
});
