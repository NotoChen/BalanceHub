import assert from "node:assert/strict";
import test from "node:test";
import type { ProviderApiKeyOption, ProviderInput, ProviderProtocolDetectionResult } from "../src/stores/provider-types.ts";
import { completedCredentials, deferred, keyFixture, providerEditorFixture, settle } from "./helpers/provider-editor-fixture.ts";
import { loadSource } from "./helpers/load-source.ts";
import * as display from "../src/utils/provider-display.ts";

test("saving NewAPI and Sub2API credentials completes missing fields before one final save", async (t) => {
  for (const protocol of ["newApi", "sub2Api"] as const) {
    const context = providerEditorFixture(t);
    context.configure(protocol);
    await settle();
    await context.editor.saveProvider();
    assert.equal(context.completionCalls(), 1);
    assert.equal(context.protocolProbeCalls(), 0);
    assert.equal(context.writes.length, 1);
    assert.equal(context.writes[0].auth.apiKey, "sk-fixture-key");
    assert.equal(context.writes[0].auth.apiKeyTokenId, "fixture-key-1");
    if (protocol === "newApi") {
      assert.equal(context.writes[0].auth.newApiSession?.refreshCookie, "fixture-refresh-cookie");
      assert.equal(context.generationCalls(), 0, "a renewable session does not need a separate PAT");
    } else {
      assert.equal(context.writes[0].auth.accessToken, "fixture-access-token");
      assert.equal(context.writes[0].auth.refreshToken, "fixture-refresh-token");
    }
    assert.equal(context.editor.drawerVisible.value, false);
    assert.equal(context.editor.savingProvider.value, false);
  }
});

test("complete credentials and API Key-only authentication skip account completion on save", async (t) => {
  for (const protocol of ["newApi", "sub2Api", "api"] as const) {
    const context = providerEditorFixture(t);
    context.configure(protocol);
    const input = JSON.parse(JSON.stringify(context.editor.draftProvider)) as ProviderInput;
    Object.assign(context.editor.draftProvider, completedCredentials(input).input);
    await settle();
    await context.editor.saveProvider();
    assert.equal(context.completionCalls(), 0);
    assert.equal(context.writes.length, 1);
    assert.equal(context.editor.drawerVisible.value, false);
  }
});

test("duplicate save clicks share completion and do not submit partial credentials", async (t) => {
  const pending = deferred<ReturnType<typeof completedCredentials>>();
  let submitted!: ProviderInput;
  const context = providerEditorFixture(t, { complete: (input) => { submitted = input; return pending.promise; } });
  await settle();
  const saving = context.editor.saveProvider();
  await context.editor.saveProvider();
  await settle();
  assert.equal(context.completionCalls(), 1);
  assert.equal(context.writes.length, 0);
  assert.equal(context.editor.savingProvider.value, true);
  pending.resolve(completedCredentials(submitted));
  await saving;
  assert.equal(context.writes.length, 1);
});

test("closing during completion releases the editor and ignores results for a previous draft", async (t) => {
  const pending = deferred<ReturnType<typeof completedCredentials>>();
  let submitted!: ProviderInput;
  const context = providerEditorFixture(t, { complete: (input) => { submitted = input; return pending.promise; } });
  await settle();
  const saving = context.editor.saveProvider();
  await settle();
  context.editor.drawerVisible.value = false;
  assert.equal(context.editor.savingProvider.value, false);
  assert.equal(context.editor.credentialAssistantBusy.value, false);
  context.editor.openAddProvider();
  context.configure();
  context.editor.draftProvider.auth.loginUsername = "another-fixture-user";
  pending.resolve(completedCredentials(submitted));
  await saving;
  assert.equal(context.writes.length, 0);
  assert.equal(context.editor.draftProvider.auth.loginUsername, "another-fixture-user");
  assert.equal(context.editor.draftProvider.auth.apiKey, "");
  assert.equal(context.editor.drawerVisible.value, true);
});

test("a completion failure preserves the draft, explains the failed save and allows retry", async (t) => {
  let fail = true;
  const context = providerEditorFixture(t, { complete: async (input) => {
    if (fail) throw new Error("登录失败，请核对密码");
    return completedCredentials(input);
  } });
  context.editor.draftProvider.identity.remark = "保留备注";
  await settle();
  await context.editor.saveProvider();
  assert.equal(context.writes.length, 0);
  assert.match(context.editor.providerSaveError.value, /登录失败/);
  assert.equal(context.editor.savingProvider.value, false);
  assert.equal(context.editor.credentialAssistantBusy.value, false);
  assert.equal(context.editor.draftProvider.identity.remark, "保留备注");
  assert.equal(context.editor.drawerVisible.value, true);
  fail = false;
  await context.editor.saveProvider();
  assert.equal(context.writes.length, 1);
});

test("closing during protocol preparation prevents credential completion from starting for a new editor", async (t) => {
  const pending = deferred<ProviderProtocolDetectionResult>();
  const context = providerEditorFixture(t, { detect: () => pending.promise });
  await settle();
  context.editor.protocolSelectionSource.value = "auto";
  context.editor.protocolSelectionBaseUrl.value = "";
  const saving = context.editor.saveProvider();
  await settle();
  assert.equal(context.protocolProbeCalls(), 1);
  context.editor.drawerVisible.value = false;
  context.editor.openAddProvider();
  context.configure();
  pending.resolve({ detectedProtocol: "newApi" } as ProviderProtocolDetectionResult);
  await saving;
  assert.equal(context.completionCalls(), 0);
  assert.equal(context.writes.length, 0);
  assert.equal(context.editor.drawerVisible.value, true);
  assert.equal(context.editor.savingProvider.value, false);
});

test("completion timeout unlocks save and a late result cannot silently save or overwrite fields", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const pending = deferred<ReturnType<typeof completedCredentials>>();
  let submitted!: ProviderInput;
  const context = providerEditorFixture(t, { complete: (input) => { submitted = input; return pending.promise; } });
  await settle();
  const saving = context.editor.saveProvider();
  await settle();
  t.mock.timers.tick(60_001);
  await saving;
  assert.equal(context.editor.savingProvider.value, false);
  assert.equal(context.editor.credentialAssistantBusy.value, false);
  assert.match(context.editor.providerSaveError.value, /补全凭据超时/);
  context.editor.draftProvider.auth.loginUsername = "edited-after-timeout";
  pending.resolve(completedCredentials(submitted));
  await settle();
  assert.equal(context.writes.length, 0);
  assert.equal(context.editor.draftProvider.auth.loginUsername, "edited-after-timeout");
  assert.equal(context.editor.draftProvider.auth.apiKey, "");
});

test("multiple Keys pause saving for a deliberate selection then continue with that Key", async (t) => {
  const keys = [keyFixture("sk-fixture-one", "key-one"), keyFixture("sk-fixture-two", "key-two")];
  const choice = deferred<ProviderApiKeyOption | null>();
  const context = providerEditorFixture(t, {
    complete: async (input) => completedCredentials(input, keys),
    chooseKey: async (available) => { assert.deepEqual(available.map((key) => key.tokenId), ["key-one", "key-two"]); return choice.promise; },
  });
  await settle();
  const saving = context.editor.saveProvider();
  await settle();
  assert.equal(context.editor.credentialAssistantState.value, "needApiKeySelection");
  assert.equal(context.writes.length, 0);
  await context.editor.saveProvider();
  choice.resolve(keys[1]);
  await saving;
  assert.equal(context.completionCalls(), 1);
  assert.equal(context.writes.length, 1);
  assert.equal(context.writes[0].auth.apiKey, "sk-fixture-two");
});

test("closing the editor also cancels its pending Key selection", async (t) => {
  const keys = [keyFixture("sk-fixture-one", "key-one"), keyFixture("sk-fixture-two", "key-two")];
  let cancelled = false;
  const context = providerEditorFixture(t, {
    complete: async (input) => completedCredentials(input, keys),
    chooseKey: (_keys, signal) => new Promise((resolve) => {
      signal.addEventListener("abort", () => { cancelled = true; resolve(null); }, { once: true });
    }),
  });
  await settle();
  const saving = context.editor.saveProvider();
  await settle();
  context.editor.drawerVisible.value = false;
  await saving;
  assert.equal(cancelled, true);
  assert.equal(context.writes.length, 0);
  assert.equal(context.editor.savingProvider.value, false);
});

test("a new renewable account is saved before its first Key is created and then updated in place", async (t) => {
  let context!: ReturnType<typeof providerEditorFixture>;
  context = providerEditorFixture(t, {
    complete: async (input) => completedCredentials(input, []),
    openKeyEditor: () => {
      assert.equal(context.writes.length, 1);
      assert.equal(context.editor.editingProviderId.value, "fixture-provider");
      assert.equal(context.editor.draftProvider.auth.newApiSession?.refreshCookie, "fixture-refresh-cookie");
      return { result: Promise.resolve(keyFixture()), close() {} };
    },
  });
  await settle();
  await context.editor.saveProvider();
  assert.equal(context.writes.length, 2);
  assert.equal(context.writes[0].id, undefined);
  assert.equal(context.writes[1].id, "fixture-provider");
  assert.equal(context.writes[1].auth.apiKey, "sk-fixture-key");
  assert.equal(context.editor.drawerVisible.value, false);
});

test("a failed Key query does not create a replacement Key or save as if completion succeeded", async (t) => {
  let opened = false;
  const context = providerEditorFixture(t, {
    complete: async (input) => ({ ...completedCredentials(input, []), steps: [{ name: "读取 API Key", ok: false, message: "HTTP 403" }] }),
    openKeyEditor: () => { opened = true; return { result: Promise.resolve(null), close() {} }; },
  });
  await settle();
  await context.editor.saveProvider();
  assert.equal(opened, false);
  assert.equal(context.writes.length, 0);
  assert.match(context.editor.providerSaveError.value, /403/);
  assert.equal(context.editor.savingProvider.value, false);
});

test("classic Cookie completion can confirm token generation while keeping the configured Key", async (t) => {
  let confirmations = 0;
  const context = providerEditorFixture(t, {
    complete: async (input) => {
      const result = completedCredentials(input);
      result.input.auth.newApiSession = null;
      result.input.auth.sessionCookie = "session=fixture-classic-cookie";
      return result;
    },
    confirm: async () => { confirmations++; return true; },
  });
  await settle();
  await context.editor.saveProvider();
  assert.equal(confirmations, 1);
  assert.equal(context.generationCalls(), 1);
  assert.equal(context.writes[0].auth.accessToken, "fixture-generated-token");
  assert.equal(context.writes[0].auth.apiKey, "sk-fixture-key");
});

test("the Key picker and token confirmation close and settle when their editor is cancelled", async () => {
  let closes = 0;
  const dialogs = loadSource<typeof import("../src/composables/provider-credential-dialogs.ts")>("composables/provider-credential-dialogs.ts", {
    "@arco-design/web-vue": { Button: {}, Modal: { open: () => ({ close: () => { closes++; } }), confirm: () => ({ close: () => { closes++; } }) } },
    "@arco-design/web-vue/es/icon": {}, "../utils/provider-display": display, "../components/RadioChoiceGroup.vue": {},
  });
  const controller = new AbortController();
  const selection = dialogs.chooseProviderApiKey([keyFixture()], controller.signal);
  const confirmation = dialogs.confirmAction("生成访问令牌", "旧令牌会失效", "生成", "warning", controller.signal);
  controller.abort();
  assert.equal(await selection, null);
  assert.equal(await confirmation, false);
  assert.equal(closes, 2);
});

test("token generation failures stay visible and never fall through to a successful save", async (t) => {
  const context = providerEditorFixture(t, {
    complete: async (input) => {
      const result = completedCredentials(input);
      result.input.auth.newApiSession = null;
      result.input.auth.sessionCookie = "session=fixture-classic-cookie";
      return result;
    },
    confirm: async () => true,
    generate: async () => { throw new Error("生成请求超时，请核对站点结果"); },
  });
  await settle();
  await context.editor.saveProvider();
  assert.equal(context.writes.length, 0);
  assert.match(context.editor.providerSaveError.value, /生成访问令牌失败/);
  assert.equal(context.editor.credentialAssistantState.value, "failed");
  assert.equal(context.editor.savingProvider.value, false);
  assert.equal(context.editor.draftProvider.auth.apiKey, "sk-fixture-key");
});

test("a created Key without readable material is reported and cannot be created again on retry", async (t) => {
  let creates = 0;
  const unreadable = { ...keyFixture("sk-fixture-key", "key-created"), keyAvailable: false, key: "" };
  const context = providerEditorFixture(t, {
    complete: async (input) => completedCredentials(input, input.auth.apiKeyOptions),
    openKeyEditor: () => { creates++; return { result: Promise.resolve(unreadable), close() {} }; },
  });
  await settle();
  await context.editor.saveProvider();
  assert.equal(context.writes.length, 1, "the account checkpoint exists before Key creation");
  assert.equal(context.editor.drawerVisible.value, true);
  assert.match(context.editor.providerSaveError.value, /未返回完整密钥/);
  await context.editor.saveProvider();
  assert.equal(creates, 1);
  assert.equal(context.editor.savingProvider.value, false);
});
