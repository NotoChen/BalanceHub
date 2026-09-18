import { launchBrowser, showBrowserWindow, bootstrapHtml, WorkerError, NeedsHuman } from "./launch.mjs";
import { LoginBrowser } from "./login.mjs";
import { AccountBrowser } from "./accounts.mjs";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const MAX_BODY_BYTES = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 30_000;
const VERIFY_TIMEOUT_MS = 180_000;
const SHIELD_COOKIE = /^(cf_clearance|__cf_bm|cf_chl_.*|acw_tc|acw_sc__v2)$/;

function keepVerificationTitle({ origin, title }) {
  if (window !== window.top || location.origin !== origin) return;
  const apply = () => {
    if (document.title === title) return;
    // Preserve the real page title for challenge detection before displaying
    // the account's station label in Chromium's window caption.
    window.__balancehubOriginalTitle = document.title;
    document.title = title;
  };
  const start = () => {
    apply();
    new MutationObserver(apply).observe(document, {
      subtree: true, childList: true, characterData: true,
    });
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
  else start();
}

export function sameOriginUrl(origin, target) {
  const url = new URL(target, origin);
  if (url.origin !== new URL(origin).origin || url.username || url.password) {
    throw new WorkerError("页面已离开目标站点，已停止发送账号凭据");
  }
  return url.href;
}

export class BrowserWorker {
  constructor(emit = () => {}) {
    this.emit = emit;
    this.context = null;
    this.page = null;
    this.origin = null;
    this.browserPid = null;
    this.closing = false;
    this.windowSession = null;
    this.windowId = null;
    this.lastPanelSize = null;
    this.autoShield = true;
  }

  async login(params) {
    if (this.context || this.loginBrowser || this.closing) throw new WorkerError("浏览器任务已启动或已取消");
    this.loginBrowser = new LoginBrowser(this.emit);
    return this.loginBrowser.run(params);
  }

  async account(params) {
    if (this.context || this.loginBrowser || this.accountBrowser || this.closing) throw new WorkerError("浏览器任务已启动或已取消");
    this.accountBrowser = new AccountBrowser(this.emit);
    return this.accountBrowser.run(params);
  }

  async showWindow() {
    if (this.closing) throw new WorkerError("登录窗口已关闭");
    return showBrowserWindow(this.accountBrowser?.context || this.loginBrowser?.context || this.context);
  }

  async open({ url, providerName, profileDir, proxy, cookies = [], executablePath, interactive = false, autoShield = true }) {
    if (this.context || this.closing) throw new WorkerError("浏览器任务已启动或已取消");
    const target = new URL(url);
    if (!["https:", "http:"].includes(target.protocol) || target.username || target.password) {
      throw new WorkerError("站点地址无效");
    }
    this.origin = target.origin;
    this.providerName = typeof providerName === "string" && providerName.trim() ? providerName.trim() : target.host;
    this.siteHost = target.host;
    this.windowTitle = `${this.providerName} · 签到验证 | BalanceHub`;
    const bootstrap = bootstrapHtml(this.windowTitle);
    this.interactive = interactive;
    this.autoShield = autoShield;
    this.context = await launchBrowser({ profileDir, executablePath, proxy, title: this.windowTitle }, this.emit);
    if (this.closing) {
      await this.context.close();
      throw new WorkerError("签到验证已取消");
    }
    // The configured account is authoritative; never inherit a previous login.
    await this.context.clearCookies({ name: "session" });
    if (!this.autoShield) {
      await this.context.clearCookies({ name: SHIELD_COOKIE });
      cookies = cookies.filter(({ name }) => !SHIELD_COOKIE.test(name));
    }
    if (cookies.length) {
      await this.context.addCookies(cookies.map(({ name, value }) => ({
        name, value, url: this.origin, httpOnly: true, secure: target.protocol === "https:",
      })));
    }
    this.page = this.context.pages()[0] || await this.context.newPage();
    this.page.setDefaultTimeout(REQUEST_TIMEOUT_MS);
    await this.page.addInitScript(keepVerificationTitle, { origin: this.origin, title: this.windowTitle });
    try {
      this.windowSession = await this.context.newCDPSession(this.page);
      this.windowId = (await this.windowSession.send("Browser.getWindowForTarget")).windowId;
    } catch {
      // Some window managers do not expose window bounds; the small window is
      // still scrollable and can be resized manually.
      await this.windowSession?.detach().catch(() => {});
      this.windowSession = null;
    }
    if (this.autoShield) {
      await this.navigate({ path: target.href });
    } else {
      // Keep a same-origin document for Turnstile without loading a page shield.
      const bootstrapRoute = (route) => route.fulfill({
        status: 200,
        contentType: "text/html; charset=utf-8",
        body: bootstrap,
      });
      await this.page.route(target.href, bootstrapRoute);
      try {
        await this.page.goto(target.href, { waitUntil: "domcontentloaded", timeout: REQUEST_TIMEOUT_MS });
      } finally {
        await this.page.unroute(target.href, bootstrapRoute);
      }
      this.assertOrigin();
    }
    return {};
  }

  assertOrigin() {
    if (!this.page || this.page.isClosed() || this.closing) {
      throw new WorkerError("验证窗口已关闭，签到已停止");
    }
    sameOriginUrl(this.origin, this.page.url());
  }

  async navigate({ path }) {
    if (!this.autoShield) throw new WorkerError("已关闭自动处理站点防护，未打开页面验证");
    const url = sameOriginUrl(this.origin, path);
    try {
      await this.page.goto(url, { waitUntil: "domcontentloaded", timeout: REQUEST_TIMEOUT_MS });
    } catch (error) {
      if (this.page.isClosed() || !String(error).includes("Timeout")) throw error;
    }
    this.assertOrigin();
    await this.waitForClearance();
    return {};
  }

  async waitForClearance() {
    const started = Date.now();
    let announced = false;
    let clicks = 0;
    while (Date.now() - started < VERIFY_TIMEOUT_MS) {
      this.assertOrigin();
      let challenged;
      try {
        challenged = await this.page.evaluate(() =>
          Boolean(window._cf_chl_opt)
          || Boolean(document.querySelector('script[src*="/cdn-cgi/challenge-platform/"]'))
          || ["Just a moment...", "正在验证…"].includes(window.__balancehubOriginalTitle ?? document.title),
        );
      } catch {
        await this.pause(500);
        continue;
      }
      if (!challenged) return;
      if (!this.autoShield) throw new WorkerError("已关闭自动处理站点防护，验证已停止");
      if (!announced) {
        this.emit({ event: "progress", phase: "verifying" });
        announced = true;
      }
      if (clicks < 2 && Date.now() - started > (clicks + 1) * 4_000) {
        if (await this.clickVerification()) clicks++;
      }
      if (Date.now() - started > 12_000 && announced !== "human") {
        if (!this.interactive) throw new NeedsHuman("需要人工完成站点验证");
        this.emit({ event: "progress", phase: "waitingHuman" });
        await this.page.bringToFront();
        announced = "human";
      }
      await this.pause(500);
    }
    throw new WorkerError("浏览器验证超时，请稍后重新签到");
  }

  async clickVerification() {
    // Only the actual Cloudflare widget is eligible. Never click site buttons.
    // Turnstile can put its iframe inside a closed shadow root. Frame ownership
    // still provides the actual element without relying on page CSS traversal.
    let box = null;
    for (const frame of this.page.frames()) {
      if (!frame.url().startsWith("https://challenges.cloudflare.com/")) continue;
      const element = await frame.frameElement().catch(() => null);
      box = await element?.boundingBox().catch(() => null);
      await element?.dispose();
      if (box) break;
    }
    if (!box || box.width < 40 || box.height < 25) return false;
    await this.page.mouse.click(box.x + 28, box.y + Math.min(box.height / 2, 32));
    return true;
  }

  async verify({ siteKey }) {
    this.assertOrigin();
    if (typeof siteKey !== "string" || !/^[A-Za-z0-9_-]{6,200}$/.test(siteKey)) {
      throw new WorkerError("站点未提供有效的 Turnstile 验证配置");
    }
    this.emit({ event: "progress", phase: "verifying" });
    // The page is a same-origin API document, with no site's check-in callback.
    // Rust remains the sole owner of the subsequent POST.
    await this.page.evaluate(async ({ siteKey, providerName, siteHost, windowTitle }) => {
      document.title = windowTitle;
      document.documentElement.lang = "zh-CN";
      document.body.replaceChildren();
      document.body.style.cssText = "margin:0;font:14px/1.5 system-ui;background:#fafafa;color:#202124";
      const panel = document.createElement("main");
      panel.id = "balancehub-verification";
      panel.style.cssText = "box-sizing:border-box;width:max-content;min-width:332px;max-width:420px;padding:16px;margin:0 auto";
      const title = document.createElement("h1");
      title.style.cssText = "margin:0 0 4px;font:600 14px/20px system-ui;overflow-wrap:anywhere";
      title.textContent = `${providerName} · 签到验证`;
      const site = document.createElement("p");
      site.className = "verification-origin";
      site.style.cssText = "margin:0 0 12px;font:12px/18px system-ui;color:#666;overflow-wrap:anywhere";
      site.textContent = siteHost;
      const hint = document.createElement("p");
      hint.className = "verification-hint";
      hint.style.cssText = "margin:10px 0 0;font:12px/18px system-ui;color:#666";
      hint.textContent = "完成后自动继续签到，关闭窗口可取消。";
      const container = document.createElement("div");
      container.id = "balancehub-turnstile";
      container.style.cssText = "min-width:300px;min-height:65px";
      panel.append(title, site, container, hint);
      document.body.append(panel);
      window.__balancehubToken = "";
      window.__balancehubVerifyError = "";
      if (!window.turnstile) {
        await new Promise((resolve, reject) => {
          const timer = setTimeout(() => reject(new Error("verification_script_timeout")), 20_000);
          const script = document.createElement("script");
          script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
          script.onload = () => { clearTimeout(timer); resolve(); };
          script.onerror = () => { clearTimeout(timer); reject(new Error("verification_script_failed")); };
          document.head.append(script);
        });
      }
      window.turnstile.render(container, {
        sitekey: siteKey,
        callback: (token) => { window.__balancehubToken = token; },
        "expired-callback": () => { window.__balancehubToken = ""; },
        "error-callback": (code) => { window.__balancehubVerifyError = String(code); },
      });
    }, { siteKey, providerName: this.providerName, siteHost: this.siteHost, windowTitle: this.windowTitle });
    this.lastPanelSize = null;
    const started = Date.now();
    let clicks = 0;
    let announced = false;
    while (Date.now() - started < VERIFY_TIMEOUT_MS) {
      this.assertOrigin();
      await this.fitVerificationWindow();
      const token = await this.page.evaluate(() => window.__balancehubToken);
      if (typeof token === "string" && token.length > 0) {
        await this.page.evaluate(() => { window.__balancehubToken = ""; });
        return { token };
      }
      if (clicks < 2 && Date.now() - started > (clicks + 1) * 4_000) {
        if (await this.clickVerification()) clicks++;
      }
      if (Date.now() - started > 12_000 && !announced) {
        if (!this.interactive) throw new NeedsHuman("需要人工完成 Turnstile 验证");
        this.emit({ event: "progress", phase: "waitingHuman" });
        await this.page.bringToFront();
        announced = true;
      }
      await this.pause(500);
    }
    throw new WorkerError("Turnstile 验证超时，请稍后重新签到");
  }

  async fitVerificationWindow() {
    if (!this.windowSession) return;
    try {
      const layout = await this.page.evaluate(() => {
        const panel = document.getElementById("balancehub-verification");
        if (!panel) return null;
        const rect = panel.getBoundingClientRect();
        return {
          width: Math.ceil(Math.max(rect.width, panel.scrollWidth)),
          height: Math.ceil(Math.max(rect.height, panel.scrollHeight)),
          frameWidth: Math.max(0, outerWidth - innerWidth),
          frameHeight: Math.max(0, outerHeight - innerHeight),
          availableWidth: screen.availWidth,
          availableHeight: screen.availHeight,
          availableLeft: screen.availLeft,
          availableTop: screen.availTop,
        };
      });
      if (!layout) return;
      const panelSize = `${layout.width}x${layout.height}`;
      // Do not fight a user's manual resize on each verification poll.
      if (panelSize === this.lastPanelSize) return;
      const { bounds } = await this.windowSession.send("Browser.getWindowBounds", { windowId: this.windowId });
      if (bounds.windowState !== "normal") return;
      const width = Math.min(layout.availableWidth, Math.max(360, layout.width + layout.frameWidth));
      const height = Math.min(layout.availableHeight, Math.max(190, layout.height + layout.frameHeight));
      await this.windowSession.send("Browser.setWindowBounds", {
        windowId: this.windowId,
        bounds: {
          width,
          height,
          left: Math.max(layout.availableLeft, Math.min(bounds.left, layout.availableLeft + layout.availableWidth - width)),
          top: Math.max(layout.availableTop, Math.min(bounds.top, layout.availableTop + layout.availableHeight - height)),
        },
      });
      this.lastPanelSize = panelSize;
    } catch {
      // Layout support must not interrupt verification or leave a retry timer.
      await this.windowSession?.detach().catch(() => {});
      this.windowSession = null;
    }
  }

  async fetch({ path, method = "GET", headers = {}, body }) {
    this.assertOrigin();
    if (!this.autoShield) await this.context.clearCookies({ name: SHIELD_COOKIE });
    const url = sameOriginUrl(this.origin, path);
    if (!["GET", "POST"].includes(method)) throw new WorkerError("不支持的签到请求方法");
    return this.page.evaluate(async (input) => {
      if (location.origin !== input.origin) throw new Error("origin_changed");
      const response = await fetch(input.url, {
        method: input.method,
        headers: input.headers,
        body: input.method === "GET" ? undefined : input.body ?? "",
        credentials: "include",
        redirect: "error",
        cache: "no-store",
        signal: AbortSignal.timeout(input.timeout),
      });
      const reader = response.body?.getReader();
      const decoder = new TextDecoder();
      let text = "";
      let bytes = 0;
      if (reader) {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          bytes += value.byteLength;
          if (bytes > input.limit) {
            await reader.cancel();
            throw new Error("response_too_large");
          }
          text += decoder.decode(value, { stream: true });
        }
      }
      text += decoder.decode();
      return {
        status: response.status,
        headers: Object.fromEntries(
          ["content-type", "cf-mitigated", "retry-after"]
            .map((name) => [name, response.headers.get(name)])
            .filter(([, value]) => value !== null),
        ),
        body: text,
        url: response.url,
      };
    }, { origin: this.origin, url, method, headers, body, limit: MAX_BODY_BYTES, timeout: REQUEST_TIMEOUT_MS });
  }

  async pause(ms) {
    await new Promise((resolve) => setTimeout(resolve, ms));
  }

  async cookies({ url } = {}) {
    this.assertOrigin();
    return { cookies: await this.context.cookies(url ? [this.origin, sameOriginUrl(this.origin, url)] : this.origin) };
  }

  async clearSession() {
    this.assertOrigin();
    await this.context.clearCookies({ name: "session" });
    return {};
  }

  async close() {
    this.closing = true;
    await this.loginBrowser?.close();
    await this.accountBrowser?.close();
    if (this.context) {
      await this.context.close().catch(() => {});
      this.context = null;
    }
    this.windowSession = null;
    return {};
  }
}

export function createWorkerRequestDispatcher(worker, emit) {
  let queue = Promise.resolve();
  const reply = async (message) => {
    const reference = message?.controlId === undefined ? { id: message?.id } : { controlId: message.controlId };
    try {
      const data = await worker[message.op](message.params || {});
      emit({ ...reference, ok: true, data });
    } catch (error) { fail(reference, error); }
  };
  function fail(reference, error) {
    emit({ ...reference, ok: false, code: error instanceof NeedsHuman ? "needsHuman" : "failed",
      error: error instanceof WorkerError ? error.message
        : String(error).match(/net::ERR_[A-Z_]+/)?.[0]
          ? "浏览器连接失败：" + String(error).match(/net::ERR_[A-Z_]+/)[0]
          : "浏览器操作失败，窗口可能已关闭或站点没有响应" });
  }
  return (line) => {
    let message;
    try {
      if (Buffer.byteLength(line) > 128 * 1024) throw new WorkerError("浏览器请求过长");
      message = JSON.parse(line);
      if (message.op === "showWindow" && Number.isSafeInteger(message.controlId) && message.controlId > 0) {
        return reply(message);
      }
      if (message.controlId !== undefined || !["login", "account", "open", "navigate", "verify", "fetch", "cookies", "clearSession", "close"].includes(message.op)) {
        throw new WorkerError("不支持的浏览器操作");
      }
      // Business requests remain serial; window controls bypass a long login.
      queue = queue.then(() => reply(message));
      return queue;
    } catch (error) {
      fail(message?.controlId === undefined ? { id: message?.id } : { controlId: message.controlId }, error);
      return Promise.resolve();
    }
  };
}

async function main() {
  const emit = (message) => process.stdout.write(JSON.stringify(message) + "\n");
  const worker = new BrowserWorker(emit);
  let stopping = false;
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    const deadline = setTimeout(() => process.exit(1), 5_000);
    deadline.unref();
    await worker.close();
    process.exit(0);
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
  input.once("close", stop);
  const dispatch = createWorkerRequestDispatcher(worker, emit);
  input.on("line", (line) => { if (!stopping) void dispatch(line); });
  await new Promise((resolve) => input.once("close", resolve));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
