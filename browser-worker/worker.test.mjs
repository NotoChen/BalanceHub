import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BrowserWorker, sameOriginUrl } from "./worker.mjs";

test("credentials are confined to the exact original origin", () => {
  assert.equal(sameOriginUrl("https://example.test", "/api/user/checkin"), "https://example.test/api/user/checkin");
  for (const target of [
    "https://other.test/api/user/checkin",
    "http://example.test/api/user/checkin",
    "https://example.test:444/api/user/checkin",
    "https://user:password@example.test/api/user/checkin",
    "//other.test/api/user/checkin",
  ]) {
    assert.throws(() => sameOriginUrl("https://example.test", target), /停止发送账号凭据/);
  }
});

test("disabled protection keeps Turnstile usable without page challenges or shield cookies", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1",
  timeout: 30_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-browser-policy-"));
  const paths = [];
  const server = createServer((request, response) => {
    paths.push(request.url);
    response.setHeader("content-type", "application/json; charset=utf-8");
    response.end(JSON.stringify({ cookie: request.headers.cookie || "" }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = "http://127.0.0.1:" + server.address().port;
  const worker = new BrowserWorker();
  try {
    await worker.open({ url: origin + "/api/status", profileDir: profile, proxy: { direct: true },
      executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE, autoShield: false,
      cookies: [{ name: "session", value: "fixture-session" }, { name: "cf_clearance", value: "fixture-shield" }],
    });
    assert.equal(await worker.page.title(), `${new URL(origin).host} · 签到验证 | BalanceHub`);
    assert.ok(!paths.includes("/api/status"), "disabled protection must not load a site challenge page");
    await assert.rejects(worker.navigate({ path: "/api/status" }), /关闭自动处理站点防护/);
    await worker.page.evaluate(() => {
      window.turnstile = { render(_container, options) { options.callback("fixture-operation-token"); } };
    });
    assert.deepEqual(await worker.verify({ siteKey: "fixture-key" }), { token: "fixture-operation-token" });
    await worker.context.addCookies([{ name: "cf_clearance", value: "later-shield", url: origin }]);
    const response = await worker.fetch({ path: "/api/user/checkin", method: "POST" });
    assert.equal(JSON.parse(response.body).cookie, "session=fixture-session");
    await worker.clearSession();
    assert.ok(!(await worker.cookies()).cookies.some((cookie) => cookie.name === "session"));
  } finally {
    await worker.close();
    await new Promise((resolve) => server.close(resolve));
    await rm(profile, { recursive: true, force: true });
  }
});

test("compact verification window fits Chinese content and preserves authenticated requests", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1",
  timeout: 45_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-browser-smoke-"));
  let submissions = 0;
  let leakedRequests = 0;
  const external = createServer((_request, response) => {
    leakedRequests++;
    response.end("{}");
  });
  await new Promise((resolve) => external.listen(0, "127.0.0.1", resolve));
  const externalUrl = "http://127.0.0.1:" + external.address().port;
  const server = createServer((request, response) => {
    if (request.url === "/redirect") {
      response.writeHead(302, { location: externalUrl });
      response.end();
    } else if (request.url === "/limited") {
      response.writeHead(429, { "content-type": "application/json", "retry-after": "60" });
      response.end('{"success":false}');
    } else if (request.method === "POST") {
      submissions++;
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({
        authorization: request.headers.authorization,
        user: request.headers["new-api-user"],
        cookie: request.headers.cookie,
      }));
    } else {
      response.setHeader("content-type", "application/json");
      response.end("{}");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = "http://127.0.0.1:" + server.address().port;
  const providerName = "本地样例 <站点>";
  const windowTitle = `${providerName} · 签到验证 | BalanceHub`;
  let verificationStarted;
  const startedVerifying = new Promise((resolve) => { verificationStarted = resolve; });
  const worker = new BrowserWorker((event) => {
    if (event.phase === "verifying") verificationStarted();
  });
  let clearance;
  let verification;
  try {
    await worker.open({
      url: origin + "/api/status", providerName, profileDir: profile, proxy: { direct: true },
      cookies: [{ name: "fixture_session", value: "fixture-cookie" }],
      executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE,
    });
    assert.equal(worker.context.pages().length, 1, "app mode must not leave an extra browser tab");
    const initialSize = await worker.page.evaluate(() => ({ width: outerWidth, height: outerHeight, frameHeight: outerHeight - innerHeight }));
    assert.ok(initialSize.width <= 440 && initialSize.height <= 400, JSON.stringify(initialSize));
    assert.ok(initialSize.frameHeight < 80, "only window decorations should remain, without tab/address/infobars");
    assert.equal(await worker.page.title(), windowTitle);
    await worker.page.evaluate(() => { document.title = "Just a moment..."; });
    await worker.page.waitForFunction((title) => document.title === title
      && window.__balancehubOriginalTitle === "Just a moment...", windowTitle);
    clearance = worker.waitForClearance();
    clearance.catch(() => {});
    await startedVerifying;
    await worker.page.evaluate(() => { document.title = "站点首页"; });
    await clearance;
    assert.equal(await worker.page.title(), windowTitle, "a site title change must not hide the station identity");
    await worker.page.evaluate(() => {
      window.turnstile = {
        render(container, options) {
          const widget = document.createElement("button");
          widget.id = "fixture-widget";
          widget.style.cssText = "display:block;box-sizing:border-box;width:300px;height:65px;font:14px system-ui";
          widget.textContent = "本地验证样例";
          widget.onclick = () => options.callback("fixture-turnstile-token");
          container.append(widget);
        },
      };
    });
    verification = worker.verify({ siteKey: "fixture-site-key" });
    verification.catch(() => {}); // Cleanup below also observes failures if an assertion interrupts the test.
    await worker.page.waitForFunction(() => {
      const panel = document.getElementById("balancehub-verification")?.getBoundingClientRect();
      return panel && outerWidth <= 380 && outerHeight <= 250
        && panel.right <= innerWidth && panel.bottom <= innerHeight
        && innerHeight - panel.bottom < 24;
    });
    assert.equal(await worker.page.title(), windowTitle);
    assert.equal(await worker.page.locator("h1").textContent(), `${providerName} · 签到验证`);
    assert.equal(await worker.page.locator(".verification-origin").textContent(), new URL(origin).host);
    assert.equal(await worker.page.locator(".verification-hint").textContent(), "完成后自动继续签到，关闭窗口可取消。");
    if (process.env.BALANCEHUB_BROWSER_SCREENSHOT) {
      await worker.page.screenshot({ path: process.env.BALANCEHUB_BROWSER_SCREENSHOT });
    }
    const compactHeight = await worker.page.evaluate(() => outerHeight);
    await worker.page.locator("#fixture-widget").evaluate((widget) => { widget.style.height = "285px"; });
    await worker.page.waitForFunction((previousHeight) => {
      const panel = document.getElementById("balancehub-verification").getBoundingClientRect();
      return outerHeight >= previousHeight + 200 && panel.bottom <= innerHeight;
    }, compactHeight);
    await worker.page.locator("#fixture-widget").evaluate((widget) => { widget.style.height = "65px"; });
    await worker.page.waitForFunction((height) => outerHeight === height, compactHeight);
    const windowSession = await worker.context.newCDPSession(worker.page);
    const { windowId } = await windowSession.send("Browser.getWindowForTarget");
    await windowSession.send("Browser.setWindowBounds", { windowId, bounds: { height: compactHeight + 80 } });
    await worker.page.waitForFunction((height) => outerHeight === height + 80, compactHeight);
    await worker.pause(650);
    assert.equal(await worker.page.evaluate(() => outerHeight), compactHeight + 80, "unchanged content must not undo a manual resize");
    await windowSession.detach();
    await worker.page.locator("#fixture-widget").click();
    assert.deepEqual(await verification, { token: "fixture-turnstile-token" });
    const headers = { authorization: "Bearer fixture-token", "new-api-user": "fixture-user" };
    const result = await worker.fetch({ path: "/api/user/checkin", method: "POST", headers });
    assert.equal(result.status, 200);
    assert.deepEqual(JSON.parse(result.body), {
      authorization: headers.authorization, user: headers["new-api-user"], cookie: "fixture_session=fixture-cookie",
    });
    const limited = await worker.fetch({ path: "/limited", headers });
    assert.equal(limited.status, 429);
    assert.equal(limited.headers["retry-after"], "60");
    await assert.rejects(worker.fetch({ path: "/redirect", headers }));
    assert.equal(leakedRequests, 0);
    assert.equal(submissions, 1);
    verification = worker.verify({ siteKey: "fixture-site-key" });
    const cancelled = assert.rejects(verification, /关闭|closed|cancel/i);
    await worker.page.locator("#fixture-widget").waitFor();
    await worker.close();
    await cancelled;
  } finally {
    await worker.close();
    await clearance?.catch(() => {});
    await verification?.catch(() => {});
    await Promise.all([
      new Promise((resolve) => server.close(resolve)),
      new Promise((resolve) => external.close(resolve)),
    ]);
    await rm(profile, { recursive: true, force: true });
  }
});
