import { readFile, writeFile, rename } from "node:fs/promises";
import { join } from "node:path";
import { launchBrowser, WorkerError } from "./launch.mjs";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const key = (cookie) => JSON.stringify([cookie.domain, cookie.path, cookie.name]);

export function identityPlatform(url) {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:") return "unknown";
    const host = parsed.hostname;
    if (host === "github.com") return "github";
    if (host === "linux.do" || host === "connect.linux.do") return "linuxDo";
  } catch { /* navigation has no usable origin yet */ }
  return "unknown";
}

async function writePrivate(path, value) {
  await writeFile(path + ".tmp", JSON.stringify(value), { mode: 0o600 });
  await rename(path + ".tmp", path);
}

export class IdentityProfile {
  constructor(profileDir, expectedPlatform = "unknown", expectedIdentity = null) {
    this.cookieFile = join(profileDir, "identity-cookies.json");
    this.stateFile = join(profileDir, "identity-state.json");
    this.expectedPlatform = expectedPlatform;
    this.expectedIdentity = expectedIdentity;
    this.platform = "unknown";
    this.identity = null;
    this.observedAt = null;
    this.mismatch = null;
    this.entryPlatform = "unknown";
    this.identities = new Map();
    this.saving = Promise.resolve();
  }

  snapshot() { return { platform: this.platform, identity: this.identity, observedAt: this.observedAt }; }

  observePlatform(url) {
    const platform = identityPlatform(url);
    if (platform === "unknown") return;
    if (this.entryPlatform === "unknown") this.entryPlatform = platform;
    // Linux DO itself can use GitHub to sign in. Intermediate identity providers
    // must not replace the selected platform's identity or reject that SSO flow.
    if (!["unknown", "other", platform].includes(this.expectedPlatform)) return;
    this.selectPlatform(platform);
  }

  selectPlatform(platform) {
    this.platform = platform;
    const observed = this.identities.get(platform);
    this.identity = observed?.identity ?? null;
    this.observedAt = observed?.observedAt ?? null;
  }

  observeIdentity(platform, value) {
    if (typeof value !== "string" || !value.trim() || value.length > 200) return;
    if (!["unknown", "other", platform].includes(this.expectedPlatform)) return;
    this.identities.set(platform, { identity: value.trim(), observedAt: Date.now() });
    this.selectPlatform(platform);
    if (this.expectedIdentity && this.expectedIdentity.toLowerCase() !== this.identity.toLowerCase()) {
      this.mismatch = "实际登录身份与所选账号不一致，请新增另一个登录账号";
    }
  }

  assertIdentity() { if (this.mismatch) throw new WorkerError(this.mismatch); }

  assertLoginPlatform(platform) {
    if (platform !== "unknown" && !["unknown", "other", platform].includes(this.expectedPlatform)) {
      throw new WorkerError("实际登录平台与所选账号不一致，请选择对应账号");
    }
  }

  async restore(context) {
    try {
      const cookies = JSON.parse(await readFile(this.cookieFile, "utf8"));
      if (!Array.isArray(cookies)) throw new Error("invalid cookies");
      const live = new Set((await context.cookies()).map(key));
      // Persistent Chromium cookies are more recent than our session snapshot.
      const missing = cookies.filter((cookie) => !live.has(key(cookie)) && (cookie.expires <= 0 || cookie.expires > Date.now() / 1000));
      if (missing.length) await context.addCookies(missing);
    } catch (error) {
      if (error.code !== "ENOENT") throw new WorkerError("无法恢复该账号的本地登录状态，可在账号管理中清除后重试");
    }
  }

  attach(context) {
    const attachPage = (page) => {
      page.on("framenavigated", (frame) => {
        if (frame === page.mainFrame()) this.observePlatform(frame.url());
      });
      page.on("domcontentloaded", () => { void this.observePage(page); });
    };
    context.pages().forEach(attachPage);
    context.on("page", attachPage);
    context.on("response", (response) => { void this.observeResponse(response).catch(() => {}); });
  }

  async observeResponse(response) {
    const url = new URL(response.url());
    if (url.origin !== "https://linux.do" || !["/session/current.json", "/session.json"].includes(url.pathname) || !response.ok()) return;
    const body = await response.body();
    if (body.length > 1024 * 1024) return;
    const payload = JSON.parse(body.toString("utf8"));
    this.observeIdentity("linuxDo", payload.current_user?.username || payload.user?.username);
  }

  async observePage(page) {
    const platform = identityPlatform(page.url());
    if (platform === "unknown") return;
    this.observePlatform(page.url());
    const identity = await page.evaluate(() => {
      if (location.hostname === "github.com") return document.querySelector('meta[name="user-login"]')?.content || null;
      if (location.hostname !== "linux.do") return null;
      try {
        const preload = document.getElementById("data-preloaded");
        const data = JSON.parse(preload?.getAttribute("data-preloaded") || preload?.textContent || "{}");
        const current = typeof data.currentUser === "string" ? JSON.parse(data.currentUser) : data.currentUser;
        return current?.username || null;
      } catch { return null; }
    }).catch(() => null);
    this.observeIdentity(platform, identity);
  }

  save(context, excluded = new Set()) {
    const next = this.saving.catch(() => {}).then(async () => {
      const cookies = (await context.cookies()).filter((cookie) => !excluded.has(key(cookie)));
      await writePrivate(this.cookieFile, cookies);
      // An unobserved navigation must not erase the last confirmed first-party
      // identity. Its original timestamp remains explicitly a past observation.
      if (this.platform !== "unknown" || this.identity) await writePrivate(this.stateFile, this.snapshot());
    });
    this.saving = next;
    return next;
  }
}

export class AccountBrowser {
  constructor(emit = () => {}, launch = launchBrowser) {
    this.emit = emit; this.launch = launch; this.context = null; this.closing = false; this.closed = false;
  }

  hasOpenWindow() {
    // On macOS, closing the last app window can leave Chromium and its context
    // alive. Page lifetime determines whether the user is still managing it.
    return !this.closed && !this.closing && this.context?.pages().some((page) => !page.isClosed());
  }

  async run({ url, accountName, profileDir, proxy, executablePath, expectedPlatform, expectedIdentity, timeoutMs = 600_000 }) {
    const target = new URL(url);
    if (target.protocol !== "https:" || identityPlatform(url) === "unknown" || target.username || target.password) throw new WorkerError("登录平台地址无效");
    this.profile = new IdentityProfile(profileDir, expectedPlatform, expectedIdentity);
    this.context = await this.launch({ profileDir, executablePath, proxy, title: `${accountName} · 登录账号 | BalanceHub`, message: "正在打开独立账号窗口…", width: 650, height: 760 }, this.emit);
    this.context.on("close", () => { this.closed = true; });
    if (this.closing) { await this.context.close(); throw new WorkerError("账号窗口已取消"); }
    await this.profile.restore(this.context);
    this.profile.attach(this.context);
    const page = this.context.pages()[0] || await this.context.newPage();
    await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30_000 }).catch(async (error) => {
      // Chromium can reject navigation just before delivering the page-close
      // event. Let that event settle before classifying a user's window close.
      if (String(error).includes("net::ERR_ABORTED") && !page.isClosed()) {
        await page.waitForEvent("close", { timeout: 1_000 }).catch(() => {});
      }
      if (this.hasOpenWindow() && !String(error).includes("Timeout")) throw error;
    });
    if (this.hasOpenWindow()) await page.bringToFront().catch(() => {});
    if (this.hasOpenWindow()) this.emit({ event: "progress", phase: "waitingHuman" });
    const deadline = Date.now() + Math.min(Math.max(timeoutMs, 1_000), 600_000);
    while (this.hasOpenWindow() && Date.now() < deadline) {
      await Promise.all(this.context.pages().map((item) => this.profile.observePage(item)));
      await this.profile.save(this.context).catch((error) => { if (!this.closed) throw error; });
      this.profile.assertIdentity();
      await pause(700);
    }
    if (this.hasOpenWindow()) throw new WorkerError("账号窗口等待超时，登录状态已保留");
    return this.profile.snapshot();
  }

  async close() {
    if (this.closing) return;
    this.closing = true;
    if (this.context && !this.closed) {
      await this.profile?.save(this.context).catch(() => {});
      await this.context.close().catch(() => {});
    }
    await this.profile?.saving.catch(() => {});
    this.context = null;
  }
}

export const profileCookieKey = key;
