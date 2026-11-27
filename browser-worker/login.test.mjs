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

test("consent is only approved for a flow that started from the relay login page", async () => {
  const fixture = (initial) => {
    let href = initial;
    let navigated;
    let evaluated = 0;
    const main = {};
    const page = {
      url: () => href,
      mainFrame: () => main,
      isClosed: () => false,
      on: (event, handler) => { if (event === "framenavigated") navigated = handler; },
      evaluate: async () => { evaluated += 1; return true; },
    };
    const browser = new LoginBrowser();
    browser.origin = "https://relay.test";
    browser.watchConsentNavigation(page);
    return {
      browser,
      page,
      go: (next) => { href = next; navigated(main); },
      evaluated: () => evaluated,
    };
  };

  // The relay itself navigates to the identity provider: its own consent flow.
  const relayFlow = fixture("https://relay.test/login");
  relayFlow.go("https://github.com/login/oauth/authorize?client_id=fixture");
  assert.equal(await relayFlow.browser.approveConsent(relayFlow.page), true);
  assert.equal(relayFlow.evaluated(), 1);

  // The platform login page in the same navigation chain stays approved.
  const chain = fixture("https://relay.test/login");
  chain.go("https://github.com/login?return_to=%2Flogin%2Foauth%2Fauthorize");
  chain.go("https://github.com/login/oauth/authorize?client_id=fixture");
  assert.equal(await chain.browser.approveConsent(chain.page), true);

  // Linux DO renders both the authorization and approval pages on its own
  // hosts; the family chain must survive that hop.
  const linuxDo = fixture("https://relay.test/login");
  linuxDo.go("https://connect.linux.do/oauth2/authorize?client_id=fixture");
  linuxDo.go("https://connect.linux.do/oauth2/approve/fixture");
  assert.equal(await linuxDo.browser.approveConsent(linuxDo.page), true);

  // A page that was not navigated by the relay cannot get its OAuth app
  // approved, even though the profile holds a platform session.
  const thirdParty = fixture("https://news.test/article");
  thirdParty.go("https://github.com/login/oauth/authorize?client_id=attacker");
  assert.equal(await thirdParty.browser.approveConsent(thirdParty.page), false);
  assert.equal(thirdParty.evaluated(), 0);

  // Leaving the identity-provider family ends the trusted chain.
  const crossRealm = fixture("https://relay.test/login");
  crossRealm.go("https://connect.linux.do/oauth2/authorize?client_id=fixture");
  crossRealm.go("https://github.com/login/oauth/authorize?client_id=attacker");
  assert.equal(await crossRealm.browser.approveConsent(crossRealm.page), false);
  assert.equal(crossRealm.evaluated(), 0);
});

test("prefixed relay deployments only treat endpoints under their base path as logins", () => {
  const browser = new LoginBrowser();
  browser.origin = "https://relay.test";
  browser.basePath = "/relay";
  assert.equal(browser.relativePath("https://relay.test/relay/api/user/login"), "/api/user/login");
  assert.equal(browser.isAuth("https://relay.test/relay/api/user/login"), true);
  assert.equal(browser.isAuth("https://relay.test/relay/api/user/auth/refresh"), true);
  assert.equal(browser.isAuth("https://relay.test/relay/api/oauth/github"), true);
  assert.equal(browser.isAuth("https://relay.test/relay/api/user/self"), false);
  assert.equal(browser.loginMechanism("https://relay.test/relay/api/user/login", "POST"), "password");
  assert.equal(browser.loginMechanism("https://relay.test/relay/api/oauth/github", "GET"), "oauth");
  assert.equal(browser.loginMechanism("https://relay.test/relay/api/oauth/state", "GET"), "unknown");
  // 同源但不在中转站前缀下的路径，即使切片后长度刚好对上，也不是登录接口。
  assert.equal(browser.isAuth("https://relay.test/other/api/user/login"), false);
  assert.equal(browser.isAuth("https://relay.test/relayx/api/user/login"), false);
  assert.equal(browser.loginMechanism("https://relay.test/other/api/user/login", "POST"), "unknown");

  const root = new LoginBrowser();
  root.origin = "https://relay.test";
  assert.equal(root.relativePath("https://relay.test/api/user/self"), "/api/user/self");
  assert.equal(root.isAuth("https://relay.test/api/user/login"), true);
  assert.equal(root.loginMechanism("https://relay.test/api/oauth/linuxdo", "GET"), "oauth");
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

test("headless relogin keeps a window.open OAuth flow inside the controlled profile", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 40_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-headless-oauth-"));
  let port;
  const server = createServer((request, response) => {
    const url = new URL(request.url, `http://${request.headers.host}`);
    const html = (body) => { response.setHeader("content-type", "text/html; charset=utf-8"); response.end(body); };
    const json = (data) => { response.setHeader("content-type", "application/json"); response.end(JSON.stringify({ success: true, data })); };
    if (url.hostname === "idp.localhost" && url.pathname === "/authorize") {
      response.writeHead(302, { location: url.searchParams.get("redirect_uri") });
      return response.end();
    }
    if (url.pathname === "/login") {
      return html(`<button id="oauth">使用 LinuxDO 继续</button><script>
        document.getElementById('oauth').onclick=()=>window.open(
          'http://idp.localhost:${port}/authorize?redirect_uri=' + encodeURIComponent(location.origin + '/oauth/oidc'));
      </script>`);
    }
    if (url.pathname === "/oauth/oidc") {
      return html(`<script>fetch('/api/oauth/linuxdo?code=fixture').then(r=>r.json()).then(r=>localStorage.setItem('user',JSON.stringify(r.data)))</script>正在完成登录`);
    }
    if (url.pathname === "/api/oauth/linuxdo") {
      response.setHeader("set-cookie", "session=relay-session; Path=/; HttpOnly; SameSite=Lax");
      return json({ id: 42, username: "fixture" });
    }
    if (url.pathname === "/api/user/self") return json({ id: 42, username: "fixture" });
    response.writeHead(404).end();
  });
  await new Promise((resolve) => server.listen(0, "0.0.0.0", resolve));
  port = server.address().port;
  const browser = new LoginBrowser();
  let extraPages = 0;
  try {
    const running = browser.run({
      url: `http://127.0.0.1:${port}`, providerName: "本地无窗口登录样例", profileDir: profile,
      executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE, proxy: { direct: true },
      expectedPlatform: "linuxDo", expectedUserId: "42", requireFreshLogin: true, headless: true, timeoutMs: 20_000,
    });
    running.catch(() => {});
    for (let attempt = 0; !browser.context; attempt++) {
      if (attempt > 200) throw new Error("browser launch timed out");
      await Promise.race([new Promise((resolve) => setTimeout(resolve, 50)), running]);
    }
    browser.context.on("page", () => { extraPages++; });
    const result = await running;
    assert.equal(result.user.id, "42");
    assert.equal(result.platform, "linuxDo");
    assert.match(result.cookieHeader, /session=relay-session/);
    assert.equal(extraPages, 0, "window.open must not create or hand off a second window");
  } finally {
    await browser.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(profile, { recursive: true, force: true });
  }
});
