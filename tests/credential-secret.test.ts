import assert from "node:assert/strict";
import test from "node:test";
import { createSecretController, type SecretView } from "../src/utils/credential-secret.ts";

test("closing or switching an account prevents late secrets from becoming visible or copied", async () => {
  let resolve!: (value: string) => void;
  const request = new Promise<string>((done) => { resolve = done; });
  let state: SecretView | null = null;
  let copies = 0;
  const controller = createSecretController(() => request, async () => { copies++; }, (next) => { state = next; });
  const pending = controller.copy();
  controller.reset();
  resolve("fixture-only-secret");
  await pending;
  assert.equal(copies, 0);
  assert.deepEqual(state, { value: "", revealed: false, pending: false, copied: false, error: "" });
});

test("explicit reveal is reversible and hide discards plaintext", async () => {
  let state = { value: "", revealed: false, pending: false, copied: false, error: "" };
  const controller = createSecretController(async () => "fixture-secret", async () => {}, (next) => { state = next; });
  assert.equal(state.value, "");
  await controller.reveal();
  assert.equal(state.value, "fixture-secret");
  assert.equal(state.revealed, true);
  await controller.reveal();
  assert.equal(state.value, "");
  assert.equal(state.revealed, false);
});

test("failed and timed out credential reads release pending state without retaining plaintext", async () => {
  let state = { value: "", revealed: false, pending: false, copied: false, error: "" };
  const failed = createSecretController(async () => { throw new Error("fixture failure"); }, async () => {}, (next) => { state = next; });
  await failed.reveal();
  assert.equal(state.pending, false); assert.match(state.error, /fixture failure/); assert.equal(state.value, "");
  const timed = createSecretController(() => new Promise(() => {}), async () => {}, (next) => { state = next; }, 5);
  await timed.reveal();
  assert.equal(state.pending, false); assert.match(state.error, /超时/); assert.equal(state.value, "");
});
