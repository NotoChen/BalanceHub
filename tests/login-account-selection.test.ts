import assert from "node:assert/strict";
import test from "node:test";
import { createLoginAccountSelection, type LoginAccountSelectionView } from "../src/utils/login-account-selection.ts";

const target = { name: "测试站点", baseUrl: "https://relay.example.test", previousAccountId: "A" };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
function setup(create: () => Promise<{ id: string }>, timeoutMs = 1000) {
  let state: LoginAccountSelectionView = { target: null, creating: false, error: "" };
  const selection = createLoginAccountSelection({ create, changed: (next) => { state = next; }, timeoutMs });
  return { selection, state: () => state };
}

test("creating an account completes the original site choice without a separate platform task", async () => {
  const account = deferred<{ id: string }>();
  const { selection, state } = setup(() => account.promise);
  const choice = selection.open(target);
  const creating = selection.create("B", "linuxDo");
  assert.deepEqual(state().target, target);
  assert.equal(state().creating, true);
  account.resolve({ id: "B" });
  await creating;
  assert.equal(await choice, "B");
  assert.equal(state().target, null);
  assert.equal(state().creating, false);
});

test("closing a picker releases it while creation is pending and ignores the late account", async () => {
  const account = deferred<{ id: string }>();
  const { selection, state } = setup(() => account.promise);
  const abandoned = selection.open(target);
  const creating = selection.create("B", "linuxDo");
  selection.cancel();
  assert.equal(await abandoned, null);
  assert.equal(state().creating, false);
  const nextTarget = { ...target, name: "另一个站点", previousAccountId: null };
  const next = selection.open(nextTarget);
  account.resolve({ id: "B" });
  await creating;
  assert.deepEqual(state().target, nextTarget);
  selection.confirm("A");
  assert.equal(await next, "A");
});

test("failed and timed out creation release pending state without accepting an account", async () => {
  const failed = setup(async () => { throw new Error("保存失败"); });
  const choice = failed.selection.open(target);
  await failed.selection.create("B", "linuxDo");
  assert.equal(failed.state().creating, false);
  assert.match(failed.state().error, /保存失败/);
  failed.selection.confirm("A");
  assert.equal(await choice, "A");

  const account = deferred<{ id: string }>();
  const timed = setup(() => account.promise, 5);
  const timedChoice = timed.selection.open(target);
  await timed.selection.create("B", "linuxDo");
  assert.equal(timed.state().creating, false);
  assert.match(timed.state().error, /超时/);
  account.resolve({ id: "B" });
  await Promise.resolve();
  assert.deepEqual(timed.state().target, target);
  timed.selection.cancel();
  assert.equal(await timedChoice, null);
});

test("switching site selection resolves the old choice without transferring its account", async () => {
  const { selection } = setup(async () => ({ id: "B" }));
  const old = selection.open(target);
  const current = selection.open({ ...target, name: "新站点", previousAccountId: null });
  assert.equal(await old, null);
  selection.confirm("B");
  assert.equal(await current, "B");
});
