import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { effectScope, ref } from "vue";
import { useProviderSave } from "../src/composables/useProviderSave.ts";
import { emptyDraft } from "../src/utils/provider-input.ts";
import type { Provider, ProviderSaveResult } from "../src/stores/provider-types.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
async function settle() { for (let index = 0; index < 8; index++) await Promise.resolve(); }
const saved = (id = "saved"): ProviderSaveResult => ({ saved: true, provider: { identity: { id } } as Provider, conflict: null });

function harness(t: TestContext, overrides: Partial<Parameters<typeof useProviderSave>[0]> = {}) {
  const visible = ref(true);
  const session = ref(1);
  const draft = emptyDraft();
  const accepted: string[] = [];
  const completed: string[] = [];
  let calls = 0;
  const scope = effectScope();
  const flow = scope.run(() => useProviderSave({ visible, session, input: () => draft, prepare: async () => {}, canSave: () => true,
    save: async () => { calls++; return saved(); }, resolveConflict: async () => "cancel",
    accept: (provider) => { accepted.push(provider.identity.id); }, completed: (provider) => { completed.push(provider.identity.id); }, ...overrides }))!;
  t.after(() => scope.stop());
  return { flow, visible, session, draft, accepted, completed, calls: () => calls };
}

test("save acknowledgement closes the editor without awaiting unrelated refresh work", async (t) => {
  const background = deferred<void>();
  let finished = false;
  const context = harness(t, { completed: () => { void background.promise.then(() => { finished = true; }); } });
  await context.flow.run();
  assert.equal(context.visible.value, false);
  assert.equal(context.flow.saving.value, false);
  assert.equal(finished, false);
  assert.deepEqual(context.accepted, ["saved"]);
  background.resolve();
});

test("duplicate save clicks do not resubmit and a closed editor cannot close a new draft", async (t) => {
  const old = deferred<ProviderSaveResult>();
  const fresh = deferred<ProviderSaveResult>();
  let calls = 0;
  const context = harness(t, { save: () => ++calls === 1 ? old.promise : fresh.promise });
  const first = context.flow.run();
  await context.flow.run();
  await settle();
  assert.equal(calls, 1);
  context.visible.value = false;
  assert.equal(context.flow.saving.value, false);
  context.session.value++;
  context.visible.value = true;
  const second = context.flow.run();
  old.resolve(saved("old"));
  await first;
  assert.equal(context.visible.value, true);
  assert.equal(context.flow.saving.value, true);
  assert.deepEqual(context.accepted, []);
  fresh.resolve(saved("fresh"));
  await second;
  assert.deepEqual(context.completed, ["fresh"]);
});

test("save failures and timeouts retain the editable draft and release the busy state", async (t) => {
  const failed = harness(t, { save: async () => { throw new Error("无法写入"); } });
  failed.draft.identity.remark = "保留此修改";
  await failed.flow.run();
  assert.equal(failed.flow.error.value, "无法写入");
  assert.equal(failed.flow.saving.value, false);
  assert.equal(failed.visible.value, true);
  assert.equal(failed.draft.identity.remark, "保留此修改");
  const pending = deferred<ProviderSaveResult>();
  const expired = harness(t, { save: () => pending.promise, timeoutMs: 5 });
  await expired.flow.run();
  assert.equal(expired.flow.saving.value, false);
  assert.match(expired.flow.error.value, /保存响应超时/);
  pending.resolve(saved());
  await settle();
  assert.deepEqual(expired.accepted, []);
  assert.equal(expired.visible.value, true);
});

test("a duplicate confirmation applies to the captured draft and cannot retry after close", async (t) => {
  const decision = deferred<"overwrite">();
  let calls = 0;
  const context = harness(t, { save: async () => { calls++; return { saved: false, provider: null, conflict: { kind: "sameAccount", existingProviderId: "a", existingProviderName: "已有配置" } }; }, resolveConflict: () => decision.promise });
  const pending = context.flow.run();
  await settle();
  context.visible.value = false;
  decision.resolve("overwrite");
  await pending;
  assert.equal(calls, 1);
  assert.equal(context.flow.saving.value, false);
});

test("merging an API Key preserves the newly opened credentials editor", async (t) => {
  let writes = 0;
  const visible = ref(true);
  const session = ref(1);
  const context = harness(t, { visible, session,
    save: async (_input, options) => { if (writes++ === 0) return { saved: false, provider: null, conflict: { kind: "sameUrlDifferentApiKey", existingProviderId: "existing", existingProviderName: "已有配置" } }; assert.equal(options.mergeApiKeyIntoProviderId, "existing"); return saved("existing"); },
    resolveConflict: async () => "merge", accept: (_provider, completion) => { assert.equal(completion, "mergedApiKey"); session.value++; } });
  await context.flow.run();
  assert.equal(visible.value, true);
  assert.equal(context.flow.saving.value, false);
  assert.deepEqual(context.completed, []);
});
