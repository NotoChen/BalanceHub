import assert from "node:assert/strict";
import test from "node:test";
import { createProviderLoginFlow } from "../src/utils/provider-login-flow.ts";
import type { ProviderBrowserLoginTask } from "../src/api/provider-browser-login.ts";
import { emptyDraft } from "../src/utils/provider-input.ts";

const task: ProviderBrowserLoginTask = { runId: "fixture-login", providerId: null, providerName: "fixture", loginAccountId: "fixture-account", operation: "import", phase: "waitingLogin",
  message: "等待登录", startedAt: 1, finishedAt: null, error: null, canCancel: true, canShowWindow: true };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { resolve, reject, promise };
}

function setup(start: () => Promise<ProviderBrowserLoginTask>, timeout = 1000) {
  const state = { session: 1, visible: true, pending: false, error: "", cancelled: "", started: false };
  const flow = createProviderLoginFlow({ editorSession: () => state.session, visible: () => state.visible, input: emptyDraft,
    ensureRuntime: async () => true, chooseAccount: async () => "fixture-account", cancelSelection: () => {}, start, cancel: async (id) => { state.cancelled = id; },
    close: () => { state.visible = false; }, pending: (pending) => { state.pending = pending; },
    failed: (error) => { state.error = error; }, started: () => { state.started = true; }, startupTimeoutMs: timeout });
  return { flow, state };
}

test("the editor closes after startup while human login is still pending", async () => {
  const { flow, state } = setup(async () => task);
  await flow.run();
  assert.equal(state.visible, false);
  assert.equal(state.pending, false);
  assert.equal(state.started, true);
  assert.equal(task.finishedAt, null);
});

test("closing an editor remains immediate and a late startup cannot affect a newer draft", async () => {
  const ack = deferred<ProviderBrowserLoginTask>();
  const { flow, state } = setup(() => ack.promise);
  const pending = flow.run();
  await Promise.resolve();
  await Promise.resolve();
  state.visible = false;
  flow.invalidate();
  assert.equal(state.pending, false);
  state.session++;
  state.visible = true;
  ack.resolve(task);
  await pending;
  assert.equal(state.visible, true);
  assert.equal(state.cancelled, task.runId);
  assert.equal(state.started, false);
});

test("startup failure and timeout release busy state and cancel late acknowledgements", async () => {
  const rejected = setup(async () => { throw new Error("启动失败"); });
  await rejected.flow.run();
  assert.equal(rejected.state.pending, false);
  assert.match(rejected.state.error, /启动失败/);
  const ack = deferred<ProviderBrowserLoginTask>();
  const { flow, state } = setup(() => ack.promise, 5);
  await flow.run();
  assert.equal(state.pending, false);
  assert.match(state.error, /超时/);
  ack.resolve(task);
  await Promise.resolve();
  assert.equal(state.cancelled, task.runId);
});

test("account selection is explicit and closing the editor cancels a pending choice", async () => {
  const choice = deferred<string | null>();
  let visible = true;
  let busy = false;
  let starts = 0;
  const flow = createProviderLoginFlow({ editorSession: () => 1, visible: () => visible, input: emptyDraft,
    ensureRuntime: async () => true, chooseAccount: () => choice.promise, cancelSelection: () => choice.resolve(null),
    start: async () => { starts++; return task; }, cancel: async () => {}, close: () => { visible = false; },
    pending: (value) => { busy = value; }, started: () => {}, failed: () => {} });
  const running = flow.run();
  await Promise.resolve();
  visible = false; flow.invalidate();
  assert.equal(busy, false);
  await running;
  assert.equal(starts, 0);
});

test("choosing B passes B to the backend without rewriting the saved A binding", async () => {
  const input = emptyDraft();
  input.auth.browserBinding = { accountId: "A", platform: "linuxDo", mechanism: "oauth", importedAt: 1 };
  let selected = "";
  let visible = true;
  const flow = createProviderLoginFlow({ editorSession: () => 1, visible: () => visible, input: () => input,
    ensureRuntime: async () => true, chooseAccount: async (draft) => { assert.equal(draft.auth.browserBinding?.accountId, "A"); return "B"; },
    cancelSelection: () => {}, start: async (_input, id) => { selected = id; return task; }, cancel: async () => {},
    close: () => { visible = false; }, pending: () => {}, started: () => {}, failed: (message) => assert.fail(message) });
  await flow.run();
  assert.equal(selected, "B");
  assert.equal(input.auth.browserBinding.accountId, "A");
});
