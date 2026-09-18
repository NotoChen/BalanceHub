import { bootstrapHtml, launchBrowser, WorkerError } from "./launch.mjs";
import { IdentityProfile, profileCookieKey } from "./accounts.mjs";

const AUTH_COOKIES = new Set(["session", "new_api_refresh"]);
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function loginUser(value) {
  if (!value || !["string", "number"].includes(typeof value.id) || !String(value.id).trim()) return null;
  return { id: String(value.id), username: String(value.username || ""), displayName: String(value.display_name || "") };
}

function loginTitle({ title }) {
  if (window !== window.top) return;
  const apply = () => { if (document.title !== title) document.title = title; };
  document.addEventListener("DOMContentLoaded", () => {
    apply();
    new MutationObserver(apply).observe(document, { subtree: true, childList: true, characterData: true });
  }, { once: true });
}

// Login may navigate to an identity provider. Only the relay's responses and
// cookies can become business credentials; IdP storage stays in this profile.
export class LoginBrowser {
  constructor(emit = () => {}) {
    this.emit = emit;
    this.context = null;
    this.closing = false;
    this.closed = false;
    this.auth = null;
    this.user = null;
    this.inFlightAuth = new Set();
    this.pendingResponses = new Set();
    this.handingOff = false;
    this.mechanism = "unknown";
    this.authPlatform = "unknown";
    this.responseSequence = 0;
    this.authSequence = 0;
    this.lastAuthActivity = 0;
    this.blockingNewAuth = false;
  }

  isTarget(url) {
    try { return new URL(url).origin === this.origin; } catch { return false; }
  }

  isAuth(url) {
    if (!this.isTarget(url)) return false;
    const path = new URL(url).pathname.slice(this.basePath.length);
    return path === "/api/user/login" || path.startsWith("/api/user/auth/") || path.startsWith("/api/oauth/");
  }

  async targetCookies() {
    return this.context.cookies([`${this.base}/`, `${this.base}/api/user/auth/refresh`]);
  }

  async clearBusinessCookies() {
    for (const cookie of await this.targetCookies()) {
      if (AUTH_COOKIES.has(cookie.name)) {
        await this.context.clearCookies({ name: cookie.name, domain: cookie.domain, path: cookie.path });
      }
    }
  }

  async saveIdentityCookies() {
    if (!this.context || this.closed) return;
    // Chromium can discard session cookies at graceful exit. Retain them
    // explicitly too, with owner-only permissions, so reopening a small window
    // does not silently log out an OAuth platform that uses a session cookie.
    const target = await this.targetCookies();
    const business = new Set(target.filter((cookie) => AUTH_COOKIES.has(cookie.name)).map(profileCookieKey));
    await this.profile.save(this.context, business);
  }

  async observe(response, sequence) {
    const url = response.url();
    if (!this.isTarget(url) || (!this.isAuth(url) && new URL(url).pathname !== this.basePath + "/api/user/self")) return;
    if (!response.ok() || !response.headers()["content-type"]?.includes("json")) return;
    const body = await response.body();
    if (body.length > 1024 * 1024) return;
    const payload = JSON.parse(body.toString("utf8"));
    if (payload.success !== true || !payload.data) return;
    const authPath = new URL(url).pathname.slice(this.basePath.length);
    if (authPath.startsWith("/api/oauth/") && !authPath.includes("/state")) {
      this.mechanism = "oauth";
      if (/\/github(?:\/|$)/i.test(authPath)) this.authPlatform = "github";
      if (/\/linux_?do(?:\/|$)/i.test(authPath)) this.authPlatform = "linuxDo";
    }
    else if (authPath === "/api/user/login") this.mechanism = "password";
    const data = payload.data;
    const user = loginUser(data.user || data);
    if (user) this.user = user;
    if (this.isAuth(url) && typeof data.access_token === "string" && data.access_token && data.session?.sid && user) {
      if (sequence < this.authSequence) return;
      this.authSequence = sequence;
      this.auth = {
        accessToken: data.access_token,
        accessExpiresAt: Number.isFinite(data.access_expires_at) ? data.access_expires_at : null,
        sessionId: String(data.session.sid),
      };
    }
  }

  async parkedPage(message) {
    const path = `${this.base}/__balancehub_login__?phase=${this.handingOff ? "handoff" : "opening"}`;
    const handler = (route) => route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: bootstrapHtml(this.title, message) });
    const page = this.context.pages().find((item) => !item.isClosed()) || await this.context.newPage();
    await page.route(path, handler);
    try {
      await page.goto(path, { waitUntil: "domcontentloaded", timeout: 15_000 });
      await page.evaluate(() => {
        for (const key of ["user", "token", "access_token", "refresh_token"]) localStorage.removeItem(key);
        sessionStorage.clear();
      });
    } finally {
      await page.unroute(path, handler);
    }
    return page;
  }

  assertOpen() {
    if (this.closing || this.closed || !this.context?.pages().some((page) => !page.isClosed())) {
      throw new WorkerError("登录窗口已关闭，导入已取消");
    }
  }

  async run({ url, providerName, accountName, profileDir, proxy, executablePath, expectedPlatform, expectedIdentity, loginPath = "/login", timeoutMs = 600_000 }) {
    const target = new URL(url);
    if (!["https:", "http:"].includes(target.protocol) || target.username || target.password || target.search || target.hash) {
      throw new WorkerError("中转站地址无效");
    }
    this.origin = target.origin;
    this.base = target.href.replace(/\/+$/, "");
    this.basePath = target.pathname.replace(/\/+$/, "");
    this.title = `${providerName?.trim() || target.host}${accountName ? ` · ${accountName}` : ""} · 登录并导入 | BalanceHub`;
    this.profile = new IdentityProfile(profileDir, expectedPlatform, expectedIdentity);
    this.context = await launchBrowser({ profileDir, executablePath, proxy, title: this.title,
      message: "正在打开站点登录页…", width: 520, height: 680 }, this.emit);
    this.context.on("close", () => { this.closed = true; });
    if (this.closing) {
      await this.context.close();
      throw new WorkerError("登录已取消");
    }
    await this.profile.restore(this.context);
    await this.clearBusinessCookies();
    await this.context.addInitScript(loginTitle, { title: this.title });
    // Crash-restored relay pages must never run a previous refresh chain.
    for (const page of this.context.pages()) {
      if (!page.url().startsWith("data:")) await page.goto("about:blank");
    }
    const page = await this.parkedPage("请在站点页面登录，完成后自动导入。");
    this.profile.attach(this.context);
    this.context.on("request", (request) => {
      if (this.isAuth(request.url()) && !this.blockingNewAuth) { this.inFlightAuth.add(request); this.lastAuthActivity = Date.now(); }
    });
    const settled = (request) => {
      if (this.inFlightAuth.delete(request)) this.lastAuthActivity = Date.now();
    };
    this.context.on("requestfinished", settled);
    this.context.on("requestfailed", settled);
    this.context.on("response", (response) => {
      const pending = this.observe(response, ++this.responseSequence).catch(() => {});
      this.pendingResponses.add(pending);
      void pending.finally(() => this.pendingResponses.delete(pending));
    });
    const login = ["/login", "/sign-in"].includes(loginPath) ? loginPath : "/login";
    await page.goto(this.base + login, { waitUntil: "domcontentloaded", timeout: 30_000 }).catch((error) => {
      this.assertOpen();
      this.profile.assertIdentity();
      if (!String(error).includes("Timeout")) throw error;
    });
    this.assertOpen();
    await page.bringToFront();
    this.emit({ event: "progress", phase: "waitingHuman" });
    const deadline = Date.now() + Math.max(1_000, Math.min(timeoutMs, 600_000));
    let nextSnapshot = Date.now() + 5_000;
    while (Date.now() < deadline) {
      this.assertOpen();
      const cookies = await this.targetCookies();
      if (!this.user && cookies.some((cookie) => cookie.name === "session")) {
        for (const targetPage of this.context.pages().filter((item) => this.isTarget(item.url()))) {
          const user = await targetPage.evaluate(() => {
            try { return JSON.parse(localStorage.getItem("user")); } catch { return null; }
          }).catch(() => null);
          this.user = loginUser(user) || this.user;
        }
      }
      if (this.user && (this.auth && cookies.some((cookie) => cookie.name === "new_api_refresh")
        || !this.auth && cookies.some((cookie) => cookie.name === "session"))) {
        return await this.handoff();
      }
      if (Date.now() >= nextSnapshot) {
        await this.saveIdentityCookies();
        nextSnapshot = Date.now() + 5_000;
      }
      await pause(400);
    }
    throw new WorkerError("登录等待超时，请重新点击登录并导入");
  }

  async handoff() {
    this.handingOff = true;
    // Stop new SPA refreshes, then drain the already submitted requests before
    // parking pages. Otherwise a server-side rotation could outlive our snapshot.
    await this.context.route((url) => this.isAuth(url.href), (route) => route.abort());
    this.blockingNewAuth = true;
    this.lastAuthActivity = Date.now();
    const deadline = Date.now() + 15_000;
    while (this.inFlightAuth.size || this.pendingResponses.size || Date.now() - this.lastAuthActivity < 400) {
      this.assertOpen();
      if (Date.now() > deadline) throw new WorkerError("登录响应尚未完成，请重新登录导入");
      await pause(100);
    }
    for (const page of this.context.pages()) {
      if (this.isTarget(page.url())) await page.goto("about:blank");
    }
    const page = await this.parkedPage("登录成功，正在核对账号并导入…");
    const auth = this.auth;
    const user = this.user;
    const result = await page.evaluate(async ({ base, token, userId }) => {
      const headers = token ? { authorization: `Bearer ${token}` } : { "new-api-user": userId };
      const response = await fetch(base + "/api/user/self", {
        headers, credentials: "include", redirect: "error", cache: "no-store", signal: AbortSignal.timeout(20_000),
      });
      return response.ok ? response.json() : null;
    }, { base: this.base, token: auth?.accessToken || "", userId: user.id });
    const verified = result?.success === true ? loginUser(result.data) : null;
    if (!verified || verified.id !== user.id) throw new WorkerError("未能确认目标站点账号，请重新登录导入");
    const cookies = await this.targetCookies();
    const refreshCookie = cookies.find((cookie) => cookie.name === "new_api_refresh")?.value || "";
    if (auth && !refreshCookie) throw new WorkerError("站点没有返回可续期的登录会话，请重新登录");
    const platform = this.mechanism === "password" ? "unknown"
      : this.authPlatform !== "unknown" ? this.authPlatform : this.profile.entryPlatform;
    this.profile.assertLoginPlatform(platform);
    this.profile.selectPlatform(platform);
    await this.saveIdentityCookies();
    this.profile.assertIdentity();
    if (["linuxDo", "github"].includes(this.profile.expectedPlatform) && this.mechanism === "password" && this.profile.platform === "unknown") {
      throw new WorkerError("本次使用的是站点密码登录，请选择站点账号类型或使用所选平台登录");
    }
    return {
      cookieHeader: cookies.filter((cookie) => cookie.name !== "new_api_refresh")
        .map((cookie) => `${cookie.name}=${cookie.value}`).join("; "),
      accessToken: auth?.accessToken || "", accessExpiresAt: auth?.accessExpiresAt ?? null,
      refreshCookie, sessionId: auth?.sessionId || "", user: verified,
      mechanism: platform !== "unknown" ? "oauth" : this.mechanism,
      platform: this.profile.platform, platformIdentity: this.profile.identity,
    };
  }

  async close() {
    if (this.closing) return;
    this.closing = true;
    if (this.context && !this.closed) {
      await this.saveIdentityCookies().catch(() => {});
      await this.clearBusinessCookies().catch(() => {});
      await this.context.close().catch(() => {});
    }
    this.context = null;
  }
}
