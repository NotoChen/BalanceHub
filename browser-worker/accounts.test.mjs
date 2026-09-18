import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { AccountBrowser, IdentityProfile, identityPlatform } from "./accounts.mjs";
import { launchBrowser } from "./launch.mjs";
import { LoginBrowser } from "./login.mjs";

test("platform identities require first-party hosts and mismatches reject reuse", () => {
  assert.equal(identityPlatform("https://github.com/login"), "github");
  assert.equal(identityPlatform("https://github.com.attacker.test/login"), "unknown");
  assert.equal(identityPlatform("https://relay.test/github"), "unknown");
  const profile = new IdentityProfile("/unused", "linuxDo", "account-a");
  profile.observePlatform("https://connect.linux.do/oauth2/authorize");
  profile.observeIdentity("linuxDo", "account-b");
  assert.throws(() => profile.assertIdentity(), /身份.*不一致/);
  const wrongPlatform = new IdentityProfile("/unused", "github");
  wrongPlatform.observePlatform("https://linux.do");
  assert.throws(() => wrongPlatform.assertLoginPlatform(wrongPlatform.entryPlatform), /平台.*不一致/);
});

test("Linux DO can sign in through GitHub without confusing the two platform identities", () => {
  const profile = new IdentityProfile("/unused", "linuxDo", "linuxdo-a");
  profile.observePlatform("https://linux.do/login");
  profile.observePlatform("https://github.com/login");
  profile.observeIdentity("github", "github-account");
  profile.assertIdentity();
  assert.equal(profile.identity, null);
  profile.observePlatform("https://linux.do/");
  profile.observeIdentity("linuxDo", "linuxdo-a");
  profile.assertLoginPlatform(profile.entryPlatform);
  assert.equal(profile.platform, "linuxDo");
  assert.equal(profile.identity, "linuxdo-a");
  const unknown = new IdentityProfile("/unused");
  unknown.observePlatform("https://github.com/"); unknown.observeIdentity("github", "github-account");
  unknown.observePlatform("https://linux.do/");
  assert.equal(unknown.identity, null, "an identity cannot cross platform boundaries");
});

test("restoring session-only cookies never overwrites a more recent Chromium cookie", async () => {
  const directory = await mkdtemp(join(tmpdir(), "balancehub-profile-test-"));
  try {
    const cookie = { name: "session", value: "old", domain: "idp.test", path: "/", expires: -1 };
    await writeFile(join(directory, "identity-cookies.json"), JSON.stringify([cookie, { ...cookie, name: "session-only" }]));
    let restored = [];
    const profile = new IdentityProfile(directory);
    await profile.restore({ cookies: async () => [{ ...cookie, value: "rotated" }], addCookies: async (value) => { restored = value; } });
    assert.equal(restored.length, 1);
    assert.equal(restored[0].name, "session-only");
  } finally { await rm(directory, { recursive: true, force: true }); }
});

async function accountFixture({ loading = false, platform = "github", progress = () => {} } = {}) {
  const profileDir = await mkdtemp(join(tmpdir(), "balancehub-account-window-"));
  let requested, waiting;
  const requestReceived = new Promise((resolve) => { requested = resolve; });
  const windowReady = new Promise((resolve) => { waiting = resolve; });
  const origin = platform === "linuxDo" ? "https://linux.do" : "https://github.com";
  const body = platform === "linuxDo"
    ? `<script id="data-preloaded" type="application/json">${JSON.stringify({ currentUser: JSON.stringify({ username: "fixture-account" }) })}</script><p>Fixture account</p>`
    : '<meta name="user-login" content="fixture-account"><p>Fixture account</p>';
  const browser = new AccountBrowser((event) => {
    if (event.phase === "waitingHuman") waiting();
    progress(event);
  }, async (params, emit) => {
    const context = await launchBrowser(params, emit);
    // Install the route before navigation: fixture identity/cookies never reach
    // the real platform, while window lifecycle uses an actual Chromium process.
    await context.route(origin + "/**", async (route) => {
      requested();
      if (loading) return;
      await route.fulfill({ status: 200, contentType: "text/html", headers: {
        "set-cookie": "user_session=fixture-account-session; Path=/; HttpOnly; Secure; SameSite=Lax",
      }, body });
    });
    return context;
  });
  const run = (timeoutMs = 10_000) => browser.run({ url: origin + (platform === "linuxDo" ? "/" : "/settings/applications"),
    accountName: "Fixture", profileDir, executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE,
    proxy: { direct: true }, expectedPlatform: platform, timeoutMs });
  const cleanup = async () => { await browser.close(); await rm(profileDir, { recursive: true, force: true }); };
  return { browser, profileDir, requestReceived, windowReady, run, cleanup };
}

async function within(promise, ms = 3_000) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("account window did not settle promptly")), ms);
    })]);
  } finally { clearTimeout(timer); }
}

for (const platform of ["github", "linuxDo"]) test(`closing the last ${platform} account page saves its first-party identity without waiting for the browser process to exit`, {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 20_000,
}, async () => {
  const fixture = await accountFixture({ platform });
  const running = fixture.run(); running.catch(() => {});
  try {
    await within(Promise.race([fixture.windowReady, running]), 10_000);
    for (let attempt = 0; !fixture.browser.profile.identity; attempt++) {
      if (attempt >= 100) throw new Error("fixture identity was not observed");
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    await fixture.browser.context.pages()[0].close();
    const result = await within(running);
    assert.equal(result.identity, "fixture-account");
    assert.equal(result.platform, platform);
    await fixture.browser.close();
    const saved = JSON.parse(await readFile(join(fixture.profileDir, "identity-state.json"), "utf8"));
    const cookies = JSON.parse(await readFile(join(fixture.profileDir, "identity-cookies.json"), "utf8"));
    assert.equal(saved.identity, "fixture-account");
    assert.equal(cookies.find((cookie) => cookie.name === "user_session")?.value, "fixture-account-session");
  } finally { await fixture.cleanup(); await running.catch(() => {}); }
});

test("closing an account page during loading finishes without publishing waiting-for-login", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 20_000,
}, async () => {
  let waitingCount = 0;
  const fixture = await accountFixture({ loading: true, progress: (event) => {
    if (event.phase === "waitingHuman") waitingCount++;
  } });
  const running = fixture.run(); running.catch(() => {});
  try {
    await within(Promise.race([fixture.requestReceived, running]), 10_000);
    await fixture.browser.context.pages()[0].close();
    const result = await within(running);
    assert.equal(result.identity, null);
    assert.equal(waitingCount, 0);
  } finally { await fixture.cleanup(); await running.catch(() => {}); }
});

test("account timeout retains the login snapshot and cleanup releases the browser", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 20_000,
}, async () => {
  const fixture = await accountFixture();
  const running = fixture.run(1_000); running.catch(() => {});
  try {
    await within(Promise.race([fixture.windowReady, running]), 10_000);
    let contextClosed = false;
    fixture.browser.context.on("close", () => { contextClosed = true; });
    await assert.rejects(within(running), /超时/);
    await fixture.browser.close();
    assert.equal(contextClosed, true);
    const saved = JSON.parse(await readFile(join(fixture.profileDir, "identity-state.json"), "utf8"));
    assert.equal(saved.identity, "fixture-account");
  } finally { await fixture.cleanup(); await running.catch(() => {}); }
});

test("A logs into xxx1, B logs into xxx2, then A logs into yyy1 without replacing either profile", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 90_000,
}, async () => {
  const directory = await mkdtemp(join(tmpdir(), "balancehub-multi-account-"));
  const logins = { A: 0, B: 0 };
  let port;
  const server = createServer((req, res) => {
    const url = new URL(req.url, `http://${req.headers.host}`);
    const host = url.hostname;
    const html = (body) => { res.setHeader("content-type", "text/html; charset=utf-8"); res.end(body); };
    const json = (data) => { res.setHeader("content-type", "application/json"); res.end(JSON.stringify(data)); };
    const redirect = (target) => { res.writeHead(302, { location: target }); res.end(); };
    if (host === "idp.localhost") {
      const target = url.searchParams.get("return_uri");
      const logged = /(?:^|;\s*)session=fixture-([AB])/.exec(req.headers.cookie || "")?.[1];
      if (url.pathname === "/authorize") {
        if (logged) return redirect(target + "?account=" + logged);
        return html(["A", "B"].map((account) => `<a href="/authenticate?as=${account}&return_uri=${encodeURIComponent(target)}">登录 ${account}</a>`).join(" "));
      }
      if (url.pathname === "/authenticate") {
        const account = url.searchParams.get("as"); logins[account]++;
        res.setHeader("set-cookie", `session=fixture-${account}; Path=/; HttpOnly; SameSite=Lax`);
        return redirect(target + "?account=" + account);
      }
    }
    if (url.pathname === "/login") return html(`<a id="oauth" href="http://idp.localhost:${port}/authorize?return_uri=${encodeURIComponent(url.origin + "/callback")}">平台登录</a>`);
    if (url.pathname === "/callback") return html(`<script>fetch('/api/oauth/mock?account=${url.searchParams.get("account")}')</script>`);
    if (url.pathname === "/api/oauth/mock") {
      const account = url.searchParams.get("account");
      res.setHeader("set-cookie", `session=relay-${account}-${host}; Path=/; HttpOnly; SameSite=Lax`);
      return json({ success: true, data: { id: `${account}-${host}`, username: `relay-${account}` } });
    }
    if (url.pathname === "/api/user/self") {
      const match = /(?:^|;\s*)session=relay-([AB])-/.exec(req.headers.cookie || "");
      const id = match ? `${match[1]}-${host}` : "";
      return json({ success: Boolean(id && req.headers["new-api-user"] === id), data: { id, username: "relay-user" } });
    }
    res.statusCode = 404; res.end();
  });
  await new Promise((resolve) => server.listen(0, "0.0.0.0", resolve));
  port = server.address().port;
  const browsers = [];
  try {
    await mkdir(join(directory, "A")); await mkdir(join(directory, "B"));
    for (const [account, host, mustLogin] of [["A", "xxx1.localhost", true], ["B", "xxx2.localhost", true], ["A", "yyy1.localhost", false]]) {
      let ready;
      const waiting = new Promise((resolve) => { ready = resolve; });
      const browser = new LoginBrowser((event) => { if (event.phase === "waitingHuman") ready(); });
      browsers.push(browser);
      const run = browser.run({ url: `http://${host}:${port}`, profileDir: join(directory, account), executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE, proxy: { direct: true }, timeoutMs: 20_000 });
      run.catch(() => {});
      await Promise.race([waiting, run]);
      const page = browser.context.pages()[0];
      await page.locator("#oauth").click();
      if (mustLogin) await page.getByRole("link", { name: `登录 ${account}` }).click();
      const result = await run;
      assert.equal(result.user.id, `${account}-${host}`);
      assert.equal(result.platformIdentity, null, "relay usernames are never treated as platform identities");
      assert.equal(result.mechanism, "oauth");
      await browser.close();
    }
    assert.deepEqual(logins, { A: 1, B: 1 });
    for (const account of ["A", "B"]) {
      const cookies = JSON.parse(await readFile(join(directory, account, "identity-cookies.json"), "utf8"));
      assert.equal(cookies.find((c) => c.domain === "idp.localhost" && c.name === "session")?.value, `fixture-${account}`);
      assert.equal(cookies.some((c) => c.value.startsWith("relay-")), false, "business credentials do not remain in identity snapshots");
    }
  } finally {
    for (const browser of browsers) await browser.close();
    server.closeAllConnections(); await new Promise((resolve) => server.close(resolve));
    await rm(directory, { recursive: true, force: true });
  }
});
