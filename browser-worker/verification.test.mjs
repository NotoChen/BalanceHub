import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { BrowserWorker } from "./worker.mjs";
import { NeedsHuman, WorkerCancelled, browserLaunchFailure } from "./launch.mjs";
import { renderTurnstile, verificationFailure } from "./verification.mjs";

const empty = () => ({ token: "", error: "", interactive: false, expired: false, timedOut: false, unsupported: false });

function workerWithStates(states, interactive = true) {
  const events = [];
  let focuses = 0;
  const worker = new BrowserWorker((event) => events.push(event));
  worker.origin = "https://example.test";
  worker.interactive = interactive;
  worker.page = {
    isClosed: () => false,
    url: () => "https://example.test/api/status",
    evaluate: async (fn) => {
      if (fn === renderTurnstile || fn.toString().includes('token = ""')) return;
      assert.ok(states.length, "verification must stop when its state is terminal");
      return states.shift();
    },
    bringToFront: async () => { focuses++; },
    mouse: { click: () => { throw new Error("human verification must never be clicked by the app"); } },
  };
  worker.fitVerificationWindow = async () => {};
  worker.pause = async () => {};
  return { worker, events, focuses: () => focuses };
}

test("manual verification is handed over immediately and continues only after a fresh token", async () => {
  const fixture = workerWithStates([{ ...empty(), interactive: true }, { ...empty(), interactive: true }, { ...empty(), token: "fresh-token" }]);
  assert.deepEqual(await fixture.worker.verify({ siteKey: "fixture-key" }), { token: "fresh-token" });
  assert.equal(fixture.events.filter((event) => event.phase === "waitingHuman").length, 1);
  assert.equal(fixture.focuses(), 1);
});

test("automatic verification yields as soon as the widget requests human input", async () => {
  const fixture = workerWithStates([{ ...empty(), interactive: true }], false);
  await assert.rejects(fixture.worker.verify({ siteKey: "fixture-key" }), NeedsHuman);
  assert.equal(fixture.focuses(), 0);
});

test("explicit verification errors fail promptly with their code instead of waiting three minutes", async () => {
  for (const state of [{ ...empty(), error: "600010" }, { ...empty(), unsupported: true }, { ...empty(), timedOut: true }, { ...empty(), expired: true }]) {
    const fixture = workerWithStates([state]);
    await assert.rejects(fixture.worker.verify({ siteKey: "fixture-key" }), /600010|不受|超时|过期/);
  }
});

test("closing the verification window releases a pending human interaction", async () => {
  const fixture = workerWithStates([{ ...empty(), interactive: true }]);
  fixture.worker.pause = async () => { fixture.worker.closing = true; };
  await assert.rejects(fixture.worker.verify({ siteKey: "fixture-key" }), WorkerCancelled);
});

test("full page challenges stay passive after human handover", async (t) => {
  let now = 1_000;
  t.mock.method(Date, "now", () => now);
  const fixture = workerWithStates([true, true, false]);
  fixture.worker.pause = async () => { now += 10_000; };
  await fixture.worker.waitForClearance();
  assert.equal(fixture.focuses(), 1);
  assert.equal(fixture.events.filter((event) => event.phase === "waitingHuman").length, 1);
});

test("widget callbacks cannot revive expired tokens or overwrite the next verification", async () => {
  const options = [];
  const element = () => ({ style: {}, append() {}, replaceChildren() {} });
  const context = { window: { turnstile: { render(_container, settings) { options.push(settings); return options.length; }, remove() {} } },
    document: { title: "", documentElement: {}, body: element(), head: element(), createElement: element },
    input: { siteKey: "fixture-key", providerName: "样例", siteHost: "example.test", windowTitle: "验证" },
  };
  const render = () => vm.runInNewContext("(" + renderTurnstile.toString() + ")(input)", context);
  await render();
  options[0]["before-interactive-callback"]();
  assert.equal(context.window.__balancehubVerification.interactive, true);
  options[0].callback("first-token");
  options[0]["expired-callback"]();
  assert.equal(context.window.__balancehubVerification.token, "");
  assert.match(verificationFailure(context.window.__balancehubVerification), /过期/);
  await render();
  options[0].callback("late-old-token");
  assert.equal(context.window.__balancehubVerification.token, "");
  options[1].callback("new-token");
  assert.equal(context.window.__balancehubVerification.token, "new-token");
  options[1]["error-callback"]("110200");
  assert.equal(context.window.__balancehubVerification.token, "");
  assert.match(verificationFailure(context.window.__balancehubVerification), /域名.*110200/);
  assert.equal(options[1].retry, "never");
});

test("browser launch diagnostics explain actionable failures without exposing raw process arguments", () => {
  assert.match(browserLaunchFailure("Executable doesn't exist at /fixture/chrome"), /不存在/);
  assert.match(browserLaunchFailure("error while loading shared libraries: libnss3.so"), /系统依赖/);
  assert.match(browserLaunchFailure("Timeout 30000ms exceeded"), /超时/);
  const message = browserLaunchFailure("process exited: --proxy-server=https://fixture-secret@example.test");
  assert.doesNotMatch(message, /fixture-secret|example.test/);
  assert.match(message, /启动失败/);
});
