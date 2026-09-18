import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BrowserWorker, createWorkerRequestDispatcher } from "./worker.mjs";
import { launchBrowser, WorkerError } from "./launch.mjs";

test("window controls complete while login is waiting and business requests stay serial", async () => {
  let finishLogin;
  const pending = new Promise((resolve) => { finishLogin = resolve; });
  const calls = [], replies = [];
  const dispatch = createWorkerRequestDispatcher({
    login: async () => { calls.push("login"); await pending; return {}; },
    showWindow: async () => { calls.push("show"); return { shown: true }; },
    cookies: async () => { calls.push("cookies"); return {}; },
  }, (reply) => replies.push(reply));
  const login = dispatch(JSON.stringify({ id: 1, op: "login" }));
  const queued = dispatch(JSON.stringify({ id: 2, op: "cookies" }));
  await Promise.resolve();
  await dispatch(JSON.stringify({ controlId: 1, op: "showWindow" }));
  assert.deepEqual(calls, ["login", "show"]);
  assert.deepEqual(replies, [{ controlId: 1, ok: true, data: { shown: true } }]);
  finishLogin();
  await Promise.all([login, queued]);
  assert.deepEqual(calls, ["login", "show", "cookies"]);
  assert.deepEqual(replies.slice(1).map((reply) => reply.id), [1, 2]);
});

test("a closed window returns a control error without failing the ongoing login", async () => {
  const replies = [];
  const dispatch = createWorkerRequestDispatcher({
    showWindow: async () => { throw new WorkerError("登录窗口已关闭"); },
    login: async () => ({ saved: true }),
  }, (reply) => replies.push(reply));
  await dispatch(JSON.stringify({ controlId: 3, op: "showWindow" }));
  await dispatch(JSON.stringify({ id: 8, op: "login" }));
  assert.equal(replies[0].ok, false);
  assert.match(replies[0].error, /已关闭/);
  assert.deepEqual(replies[1], { id: 8, ok: true, data: { saved: true } });
});

test("showing a real browser restores its existing minimized window and does not reopen a closed page", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 20_000,
}, async () => {
  const profileDir = await mkdtemp(join(tmpdir(), "balancehub-window-control-"));
  const worker = new BrowserWorker();
  try {
    worker.context = await launchBrowser({ profileDir, executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE,
      proxy: { direct: true }, title: "Fixture window" }, () => {});
    const pages = worker.context.pages();
    const page = pages[0];
    const cdp = await worker.context.newCDPSession(page);
    const { windowId } = await cdp.send("Browser.getWindowForTarget");
    await cdp.send("Browser.setWindowBounds", { windowId, bounds: { windowState: "minimized" } });
    assert.deepEqual(await worker.showWindow(), { shown: true });
    const window = await cdp.send("Browser.getWindowBounds", { windowId });
    assert.equal(window.bounds.windowState, "normal");
    assert.deepEqual(worker.context.pages(), pages);
    await cdp.detach();
    await page.close();
    await assert.rejects(worker.showWindow(), /已关闭/);
  } finally { await worker.close(); await rm(profileDir, { recursive: true, force: true }); }
});
