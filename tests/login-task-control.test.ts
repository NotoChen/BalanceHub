import assert from "node:assert/strict";
import test from "node:test";
import { createLoginTaskControl } from "../src/utils/login-task-control.ts";

test("a waiting window control only disables its own task and releases on failure", async () => {
  const changes: [string, boolean][] = [];
  const errors: string[] = [];
  const control = createLoginTaskControl({ changed: (id, value) => changes.push([id, value]), failed: (message) => errors.push(message) });
  await control.run("A", async () => {
    assert.equal(control.pending("A"), true);
    assert.equal(control.pending("B"), false);
    throw new Error("窗口已关闭");
  });
  assert.equal(control.pending("A"), false);
  assert.deepEqual(changes, [["A", true], ["A", false]]);
  assert.deepEqual(errors, ["窗口已关闭"]);
});

test("timeout releases the task and a late result cannot re-disable it", async () => {
  let resolve!: () => void;
  const operation = new Promise<void>((yes) => { resolve = yes; });
  const changes: boolean[] = [];
  const errors: string[] = [];
  const control = createLoginTaskControl({ changed: (_id, value) => changes.push(value), failed: (message) => errors.push(message), timeoutMs: 5 });
  await control.run("A", () => operation);
  assert.equal(control.pending("A"), false);
  assert.match(errors[0], /超时/);
  resolve();
  await Promise.resolve();
  assert.deepEqual(changes, [true, false]);
});

test("unmounting suppresses late UI changes and errors", async () => {
  let reject!: (error: Error) => void;
  const operation = new Promise<void>((_resolve, no) => { reject = no; });
  const changes: boolean[] = [];
  const control = createLoginTaskControl({ changed: (_id, value) => changes.push(value), failed: () => assert.fail("disposed view received error") });
  const running = control.run("A", () => operation);
  control.dispose();
  reject(new Error("late"));
  await running;
  assert.deepEqual(changes, [true]);
  assert.equal(control.pending("A"), false);
});
