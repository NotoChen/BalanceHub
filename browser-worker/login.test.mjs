import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LoginBrowser, loginUser } from "./login.mjs";

test("only a concrete relay user can be imported", () => {
  assert.equal(loginUser({ username: "missing-id" }), null);
  assert.deepEqual(loginUser({ id: 42, username: "fixture" }), { id: "42", username: "fixture", displayName: "" });
});

test("two relay sites reuse a session-only OAuth cookie across browser restarts and preserve separate rotating sessions", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 80_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-oauth-smoke-"));
  let identityLogins = 0;
  const rotations = { "a.localhost": 0, "b.localhost": 0 };
  const ids = { "a.localhost": 42, "b.localhost": 84 };
  let origin;
  const server = createServer((request, response) => {
    const host = request.headers.host.split(":")[0];
    const url = new URL(request.url, `http://${request.headers.host}`);
    const sendHtml = (body) => { response.setHeader("content-type", "text/html; charset=utf-8"); response.end(body); };
    const redirect = (location) => { response.writeHead(302, { location }); response.end(); };
    if (host === "idp.localhost") {
      if (url.pathname === "/authorize") {
        if ((request.headers.cookie || "").includes("session=fixture-idp")) return redirect(url.searchParams.get("redirect_uri"));
        return sendHtml(`<form method="post" action="/sign-in?redirect_uri=${encodeURIComponent(url.searchParams.get("redirect_uri"))}"><button>登录平台一次</button></form>`);
      }
      if (url.pathname === "/sign-in") {
        identityLogins++;
        // Deliberately a session cookie named "session", like the old relays.
        response.setHeader("set-cookie", "session=fixture-idp; Path=/; HttpOnly; SameSite=Lax");
        return redirect(url.searchParams.get("redirect_uri"));
      }
    }
    if (url.pathname === "/login") {
      const authorize = origin.replace("a.localhost", "idp.localhost") + "/authorize?redirect_uri=" + encodeURIComponent(`http://${request.headers.host}/oauth/mock`);
      const action = host === "b.localhost" ? `window.open(${JSON.stringify(authorize)})` : `location.href=${JSON.stringify(authorize)}`;
      return sendHtml(`<button id="oauth">第三方登录</button><script>document.getElementById('oauth').onclick=()=>{${action}}</script>`);
    }
    if (url.pathname === "/oauth/mock") {
      return sendHtml(`<script>fetch('/api/oauth/mock').then(r=>r.json()).then(()=>{
        setInterval(()=>fetch('/api/user/auth/refresh',{method:'POST'}).catch(()=>{}),120);
      });</script>正在完成登录`);
    }
    if (url.pathname === "/api/oauth/mock" || url.pathname === "/api/user/auth/refresh") {
      rotations[host]++;
      const count = rotations[host];
      response.setHeader("set-cookie", `new_api_refresh=refresh-${ids[host]}-${count}; Path=/api/user/auth; HttpOnly; SameSite=Lax`);
      response.setHeader("content-type", "application/json");
      return response.end(JSON.stringify({ success: true, data: {
        access_token: `access-${ids[host]}-${count}`, access_expires_at: Math.floor(Date.now() / 1000) + 900,
        session: { sid: `session-${ids[host]}` }, user: { id: ids[host], username: `fixture-${ids[host]}` },
      } }));
    }
    if (url.pathname === "/api/user/self") {
      response.setHeader("content-type", "application/json");
      const valid = request.headers.authorization === `Bearer access-${ids[host]}-${rotations[host]}`;
      response.statusCode = valid ? 200 : 401;
      return response.end(JSON.stringify({ success: valid, data: { id: ids[host], username: `fixture-${ids[host]}` } }));
    }
    response.statusCode = 404;
    response.end();
  });
  await new Promise((resolve) => server.listen(0, "0.0.0.0", resolve));
  origin = `http://a.localhost:${server.address().port}`;
  const input = { profileDir: profile, executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE, proxy: { direct: true }, timeoutMs: 25_000 };
  const browsers = [];
  try {
    for (const host of ["a.localhost", "b.localhost"]) {
      let waitingPath;
      let onWaiting;
      const waiting = new Promise((resolve) => { onWaiting = resolve; });
      const browser = new LoginBrowser((event) => {
        if (event.event === "progress" && event.phase === "waitingHuman") {
          waitingPath = new URL(browser.context.pages()[0].url()).pathname;
          onWaiting();
        }
      });
      browsers.push(browser);
      const running = browser.run({ ...input, url: origin.replace("a.localhost", host), providerName: "本地登录样例" });
      running.catch(() => {});
      await Promise.race([waiting, running]);
      assert.equal(waitingPath, "/login", "waiting for login must follow the real login navigation");
      const page = browser.context.pages()[0];
      await Promise.race([page.locator("#oauth").click({ timeout: 10_000 }), running.then(() => undefined)]);
      if (host === "a.localhost") await page.getByRole("button", { name: "登录平台一次" }).click();
      const result = await running;
      assert.equal(result.user.id, String(ids[host]));
      assert.equal(result.refreshCookie, `refresh-${ids[host]}-${rotations[host]}`);
      assert.equal(result.accessToken, `access-${ids[host]}-${rotations[host]}`);
      assert.equal(result.cookieHeader.includes("fixture-idp"), false, "IdP credentials never leave the browser profile");
      const cookies = await browser.context.cookies(origin.replace("a.localhost", "idp.localhost"));
      assert.equal(cookies.find((cookie) => cookie.name === "session")?.value, "fixture-idp");
      await browser.close();
    }
    assert.equal(identityLogins, 1, "the second relay must not ask for IdP credentials again");
  } finally {
    for (const browser of browsers) await browser.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(profile, { recursive: true, force: true });
  }
});

test("closing while the login page is loading never reports waiting for human login", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 20_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-oauth-loading-"));
  let onLoginRequest;
  const loginRequested = new Promise((resolve) => { onLoginRequest = resolve; });
  const server = createServer((request, response) => {
    if (request.url === "/login") {
      onLoginRequest();
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  let waitingCount = 0;
  const browser = new LoginBrowser((event) => {
    if (event.event === "progress" && event.phase === "waitingHuman") waitingCount++;
  });
  try {
    const running = browser.run({ url: `http://127.0.0.1:${server.address().port}`, profileDir: profile,
      executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE, proxy: { direct: true } });
    const cancelled = assert.rejects(running, /已关闭|已取消|closed/);
    cancelled.catch(() => {});
    await Promise.race([loginRequested, running]);
    assert.equal(waitingCount, 0);
    await browser.close();
    await cancelled;
    assert.equal(waitingCount, 0);
  } finally {
    await browser.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(profile, { recursive: true, force: true });
  }
});

test("closing a login window cancels without importing credentials", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 20_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-oauth-cancel-"));
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html"); response.end("<button>pending login</button>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const browser = new LoginBrowser();
  try {
    const running = browser.run({ url: `http://127.0.0.1:${server.address().port}`, profileDir: profile,
      executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE, proxy: { direct: true } });
    const cancelled = assert.rejects(running, /已关闭|已取消|closed/);
    for (let attempt = 0; !browser.context?.pages().length; attempt++) {
        if (attempt > 200) throw new Error("browser launch timed out");
        await Promise.race([new Promise((resolve) => setTimeout(resolve, 50)), running]);
      }
    await browser.context.pages()[0].getByRole("button").waitFor();
    await browser.close();
    await cancelled;
  } finally {
    await browser.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(profile, { recursive: true, force: true });
  }
});
