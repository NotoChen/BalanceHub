import assert from "node:assert/strict";
import test from "node:test";
import { effectScope, nextTick, reactive, ref } from "vue";
import { useProviderCheckInPolicy } from "../src/composables/useProviderCheckInPolicy.ts";
import { emptyDraft } from "../src/utils/provider-input.ts";
import type { ProviderCheckInPolicyPreview } from "../src/stores/provider-types.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 5));
const fixture = (method: "standard" | "freshLogin"): ProviderCheckInPolicyPreview => ({
  method, methodLabel: "签到方式", configurable: true, supported: true, message: "fixture",
});

test("new provider check-in settings default to automatic behavior", () => {
  assert.deepEqual(emptyDraft().automation, {
    refreshInterval: 0, checkInTime: "", checkInMethod: "auto", autoShield: true, turnstileMode: "auto",
  });
});

test("closing the editor releases policy loading while IPC is still pending", async () => {
  const pending = deferred<ProviderCheckInPolicyPreview>();
  const scope = effectScope();
  const active = ref(true);
  const draft = reactive(emptyDraft());
  const state = scope.run(() => useProviderCheckInPolicy({
    input: () => draft, active: () => active.value, preview: () => pending.promise, debounceMs: 0,
  }))!;
  await settle();
  assert.equal(state.loading.value, true);
  active.value = false;
  await nextTick();
  assert.equal(state.loading.value, false);
  pending.resolve(fixture("standard"));
  await settle();
  assert.equal(state.policy.value, null);
  scope.stop();
});

test("late previews cannot overwrite a newer method or an unmounted editor", async () => {
  const old = deferred<ProviderCheckInPolicyPreview>();
  const latest = deferred<ProviderCheckInPolicyPreview>();
  const scope = effectScope();
  const draft = reactive(emptyDraft());
  let calls = 0;
  const state = scope.run(() => useProviderCheckInPolicy({
    input: () => draft, active: () => true,
    preview: () => ++calls === 1 ? old.promise : latest.promise, debounceMs: 0,
  }))!;
  await settle();
  draft.automation.checkInMethod = "freshLogin";
  await settle();
  latest.resolve(fixture("freshLogin"));
  await settle();
  old.resolve(fixture("standard"));
  await settle();
  assert.equal(state.policy.value?.method, "freshLogin");
  scope.stop();
  assert.equal(state.loading.value, false);
  assert.equal(state.policy.value, null);
});

test("preview timeout and failure release state without disabling editing", async () => {
  for (const timeout of [false, true]) {
    const pending = deferred<ProviderCheckInPolicyPreview>();
    const scope = effectScope();
    const state = scope.run(() => useProviderCheckInPolicy({
      input: emptyDraft, active: () => true, preview: () => pending.promise, debounceMs: 0, timeoutMs: timeout ? 5 : 100,
    }))!;
    if (!timeout) {
      await settle();
      pending.reject(new Error("fixture failure"));
    }
    await new Promise((resolve) => setTimeout(resolve, 15));
    assert.equal(state.loading.value, false);
    assert.match(state.error.value, /可继续编辑/);
    pending.resolve(fixture("standard"));
    await settle();
    assert.equal(state.policy.value, null);
    scope.stop();
  }
});
