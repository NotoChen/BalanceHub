import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LoginBrowser } from "./login.mjs";
import { WorkerCancelled } from "./launch.mjs";

function response(path, data, { method = "GET", success = true, origin = "https://relay.test" } = {}) {
  return { url: () => origin + path, ok: () => true, headers: () => ({ "content-type": "application/json" }),
    body: async () => Buffer.from(JSON.stringify({ success, data })), request: () => ({ method: () => method }) };
}

test("self, OAuth state and refresh responses cannot prove a fresh login", async () => {
  const browser = new LoginBrowser();
  browser.origin = "https://relay.test";
  browser.basePath = "";
  let sequence = 0;
  for (const [path, method] of [["/api/user/self", "GET"], ["/api/oauth/state", "GET"],
    ["/api/user/auth/refresh", "POST"], ["/api/oauth/github/bind", "GET"]]) {
    await browser.observe(response(path, { id: 42, username: "fixture" }, { method }), ++sequence);
    assert.equal(browser.freshUser, null, path);
  }
  await browser.observe(response("/api/oauth/linuxdo", { id: 42 }, { origin: "https://identity.test" }), ++sequence);
  assert.equal(browser.freshUser, null, "another origin is not the relay login");
  await browser.observe(response("/api/oauth/linuxdo", { id: 42 }, { success: false }), ++sequence);
  assert.equal(browser.freshUser, null, "a rejected login is not a fresh session");
  await browser.observe(response("/api/oauth/linuxdo", { id: 42, username: "fixture" }), ++sequence);
  assert.equal(browser.freshUser.id, "42");
  assert.equal(browser.mechanism, "oauth");
  assert.equal(browser.authPlatform, "linuxDo");
});

test("automatic platform selection never clicks a settings or identity-provider page", async () => {
  const browser = new LoginBrowser();
  browser.origin = "https://relay.test";
  browser.basePath = "";
  browser.requireFreshLogin = true;
  for (const url of ["https://relay.test/settings", "https://linux.do/login"]) {
    await browser.startBoundLogin({ url: () => url, getByRole: () => assert.fail("not a relay login page") });
  }
});

function loginControls() {
  const state = { challenge: false, visible: false, enabled: false, count: 1, clicks: 0, failClick: false, closed: false };
  const browser = new LoginBrowser();
  Object.assign(browser, { origin: "https://relay.test", basePath: "", requireFreshLogin: true,
    profile: { expectedPlatform: "linuxDo" } });
  const control = {
    or: () => control, count: async () => state.count, isVisible: async () => state.visible, isEnabled: async () => state.enabled,
    click: async () => { state.clicks++; if (state.failClick) throw new Error("navigation acknowledgement interrupted"); },
  };
  const page = { url: () => "https://relay.test/login", isClosed: () => state.closed,
    evaluate: async () => state.challenge,
    getByRole: (role, { name }) => {
      assert.ok(["button", "link"].includes(role));
      assert.ok(name.test("使用 LinuxDO 继续"));
      assert.ok(name.test("使用 Linux DO 账户登录"));
      assert.ok(!name.test("人工验证") && !name.test("使用 GitHub 继续"));
      return control;
    },
  };
  return { browser, page, state };
}

test("bound login keeps waiting for delayed controls and resumes automatically after human verification", async () => {
  const { browser, page, state } = loginControls();
  await browser.startBoundLogin(page);
  state.visible = true;
  await browser.startBoundLogin(page);
  state.enabled = true;
  state.challenge = true;
  await browser.startBoundLogin(page);
  assert.equal(state.clicks, 0);
  state.challenge = false;
  await browser.startBoundLogin(page);
  await browser.startBoundLogin(page);
  assert.equal(state.clicks, 1, "a completed verification continues exactly one login flow");
});

test("ambiguous controls, cancellation and possibly delivered clicks never cause automatic resubmission", async () => {
  for (const reason of ["ambiguous", "cancelled", "closed", "uncertain"]) {
    const { browser, page, state } = loginControls();
    state.visible = true; state.enabled = true;
    if (reason === "ambiguous") state.count = 2;
    if (reason === "cancelled") browser.closing = true;
    if (reason === "closed") state.closed = true;
    if (reason === "uncertain") state.failClick = true;
    await browser.startBoundLogin(page);
    await browser.startBoundLogin(page);
    assert.equal(state.clicks, reason === "uncertain" ? 1 : 0, reason);
  }
});

const smoke = { skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1", timeout: 30_000 };

async function fixture(mode, run) {
  const directory = await mkdtemp(join(tmpdir(), "balancehub-checkin-login-"));
  const observed = { logins: 0, self: 0, oldCookieReachedLogin: false, phases: [], verificationClicks: 0, openedAt: 0, loggedInAt: 0 };
  const id = mode === "wrong-user" ? 43 : 42;
  const user = { id, username: "fixture" };
  const server = createServer((request, reply) => {
    const path = new URL(request.url, `http://${request.headers.host}`).pathname;
    const json = (data) => {
      reply.setHeader("content-type", "application/json");
      reply.end(JSON.stringify({ success: true, data }));
    };
    if (path === "/login") {
      observed.openedAt = Date.now();
      observed.oldCookieReachedLogin ||= (request.headers.cookie || "").includes("old-site-session");
      reply.setHeader("content-type", "text/html; charset=utf-8");
      if (mode === "refresh-only") return reply.end(`<script>
        fetch('/api/user/auth/refresh',{method:'POST'}).then(r=>r.json()).then(r=>{
          localStorage.setItem('user',JSON.stringify(r.data));fetch('/api/user/self');
        });</script>原有会话已恢复`);
      if (mode === "waiting") return reply.end("<p>请登录账号</p>");
      const control = mode === "link" ? '<a href="#" id="login">使用 LinuxDO 继续</a>' : '<button id="login">使用 LinuxDO 继续</button>';
      return reply.end(`<div id="controls"></div><input type="checkbox" aria-label="人工验证" onchange="fetch('/unexpected-verification')">
        <script>
          function ready() {
            delete window._cf_chl_opt;
            document.getElementById('controls').innerHTML=${JSON.stringify(control)};
            document.getElementById('login').onclick=(event)=>{
              event.preventDefault();
              fetch('/api/oauth/linuxdo?code=fixture').then(r=>r.json()).then(r=>localStorage.setItem('user',JSON.stringify(r.data)));
            };
          }
          ${mode === "challenge" ? "window._cf_chl_opt = {};" : ""}
          ${["delayed-control", "challenge"].includes(mode) ? "setTimeout(ready, 3_000);" : "ready();"}
        </script>`);
    }
    if (path === "/api/oauth/linuxdo" || path === "/api/user/auth/refresh") {
      if (path === "/api/oauth/linuxdo") { observed.logins++; observed.loggedInAt = Date.now(); }
      reply.setHeader("set-cookie", "session=fresh-site-session; Path=/; HttpOnly; SameSite=Lax");
      return json(user);
    }
    if (path === "/api/user/self") {
      observed.self++;
      return json(user);
    }
    if (path === "/unexpected-verification") { observed.verificationClicks++; return reply.end(); }
    reply.writeHead(404).end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}`;
  await writeFile(join(directory, "identity-cookies.json"), JSON.stringify([
    { name: "session", value: "old-site-session", domain: "127.0.0.1", path: "/", expires: -1,
      httpOnly: true, secure: false, sameSite: "Lax" },
  ]), { mode: 0o600 });
  const browser = new LoginBrowser((event) => { if (event.event === "progress") observed.phases.push(event.phase); });
  const input = { url, profileDir: directory, executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE,
    proxy: { direct: true }, providerName: "本地重新登录样例", expectedPlatform: "linuxDo",
    expectedUserId: "42", requireFreshLogin: true, timeoutMs: mode === "refresh-only" ? 1_000 : 10_000 };
  try { await run(browser, input, observed); }
  finally {
    await browser.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    await rm(directory, { recursive: true, force: true });
  }
}

test("a bound OAuth check-in clears the old relay cookie and proves a new same-user login", smoke, async () => {
  await fixture("success", async (browser, input, observed) => {
    const result = await browser.run(input);
    assert.equal(result.freshLogin, true);
    assert.equal(result.user.id, "42");
    assert.equal(result.platform, "linuxDo");
    assert.match(result.cookieHeader, /session=fresh-site-session/);
    assert.equal(observed.logins, 1);
    assert.equal(observed.self, 1);
    assert.equal(observed.oldCookieReachedLogin, false);
    assert.ok(observed.phases.includes("loggingIn"));
    assert.equal(observed.phases.at(-1), "verifyingResult");
    assert.equal(observed.verificationClicks, 0);
  });
});

for (const mode of ["delayed-control", "challenge", "link"]) {
  test(`fresh login automatically continues with ${mode} without touching verification controls`, smoke, async () => {
    await fixture(mode, async (browser, input, observed) => {
      const result = await browser.run(input);
      assert.equal(result.freshLogin, true);
      assert.equal(observed.logins, 1);
      assert.equal(observed.verificationClicks, 0);
      if (mode !== "link") assert.ok(observed.loggedInAt - observed.openedAt >= 2_800);
      if (mode === "challenge") assert.ok(observed.phases.includes("waitingHuman"));
    });
  });
}

test("independent relay windows can wait and complete concurrently", smoke, async () => {
  await Promise.all(["delayed-control", "challenge"].map((mode) => fixture(mode, async (browser, input, observed) => {
    const result = await browser.run(input);
    assert.equal(result.user.id, "42");
    assert.equal(observed.logins, 1);
    assert.equal(observed.verificationClicks, 0);
  })));
});

test("a restored session and refresh response time out without a fresh login proof", smoke, async () => {
  await fixture("refresh-only", async (browser, input, observed) => {
    await assert.rejects(browser.run(input), /重新登录等待超时/);
    assert.equal(observed.logins, 0);
    assert.equal(observed.oldCookieReachedLogin, false);
    assert.deepEqual(observed.phases, ["waitingLogin"]);
  });
});

test("browser check-in refuses a different relay user after OAuth", smoke, async () => {
  await fixture("wrong-user", async (browser, input, observed) => {
    await assert.rejects(browser.run(input), /原站点账号不一致/);
    assert.equal(observed.logins, 1);
  });
});

test("closing a pending fresh-login window settles without submitting or returning credentials", smoke, async () => {
  await fixture("waiting", async (browser, input, observed) => {
    const running = browser.run(input);
    const cancelled = assert.rejects(running, WorkerCancelled);
    for (let attempt = 0; !observed.phases.includes("waitingLogin"); attempt++) {
      if (attempt > 200) throw new Error("login window did not open");
      await Promise.race([new Promise((resolve) => setTimeout(resolve, 50)), running]);
    }
    await browser.context.pages()[0].close();
    await cancelled;
    assert.equal(observed.logins, 0);
  });
});
