import { chromium } from "playwright-core";
import { mkdir, chmod } from "node:fs/promises";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const MAX_BODY_BYTES = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 30_000;
const VERIFY_TIMEOUT_MS = 180_000;

class WorkerError extends Error {}
class NeedsHuman extends WorkerError {}

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
  }

  async open({ url, profileDir, proxy, cookies = [], executablePath, interactive = false }) {
    if (this.context || this.closing) throw new WorkerError("浏览器任务已启动或已取消");
    const target = new URL(url);
    if (!["https:", "http:"].includes(target.protocol) || target.username || target.password) {
      throw new WorkerError("站点地址无效");
    }
    this.origin = target.origin;
    this.interactive = interactive;
    if (!executablePath) throw new WorkerError("没有可用的签到浏览器，请在 App 中安装组件");
    await mkdir(profileDir, { recursive: true, mode: 0o700 });
    if (process.platform !== "win32") await chmod(profileDir, 0o700);
    this.context = await chromium.launchPersistentContext(profileDir, {
      executablePath,
      headless: false,
      viewport: null,
      timeout: 30_000,
      ignoreDefaultArgs: ["--enable-automation"],
      args: [
        "--disable-blink-features=AutomationControlled",
        "--disable-session-crashed-bubble",
        ...(proxy?.direct ? ["--no-proxy-server"] : []),
      ],
      proxy: proxy?.server ? {
        server: proxy.server,
        bypass: proxy.bypass || undefined,
        username: proxy.username || undefined,
        password: proxy.password || undefined,
      } : undefined,
    });
    if (this.closing) {
      await this.context.close();
      throw new WorkerError("签到验证已取消");
    }
    const cdp = await this.context.browser().newBrowserCDPSession();
    const processes = await cdp.send("SystemInfo.getProcessInfo");
    this.browserPid = processes.processInfo.find((item) => item.type === "browser")?.id ?? null;
    await cdp.detach();
    this.emit({ event: "browserStarted", browserPid: this.browserPid });
    if (cookies.length) {
      await this.context.addCookies(cookies.map(({ name, value }) => ({
        name, value, url: this.origin, httpOnly: true, secure: target.protocol === "https:",
      })));
    }
    this.page = this.context.pages()[0] || await this.context.newPage();
    this.page.setDefaultTimeout(REQUEST_TIMEOUT_MS);
    await this.navigate({ path: target.href });
    return {};
  }

  assertOrigin() {
    if (!this.page || this.page.isClosed() || this.closing) {
      throw new WorkerError("验证窗口已关闭，签到已停止");
    }
    sameOriginUrl(this.origin, this.page.url());
  }

  async navigate({ path }) {
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
          || ["Just a moment...", "正在验证…"].includes(document.title),
        );
      } catch {
        await this.pause(500);
        continue;
      }
      if (!challenged) return;
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
    await this.page.evaluate(async ({ siteKey }) => {
      document.title = "BalanceHub · 签到验证";
      document.body.replaceChildren();
      document.body.style.cssText = "font:16px system-ui;padding:48px;line-height:1.8;background:#f5f5f5;color:#202124";
      const title = document.createElement("h2");
      title.textContent = "完成验证后将自动继续签到";
      const hint = document.createElement("p");
      hint.textContent = "请保持此窗口打开。需要时，点击下方验证框。";
      const container = document.createElement("div");
      container.id = "balancehub-turnstile";
      document.body.append(title, hint, container);
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
    }, { siteKey });
    const started = Date.now();
    let clicks = 0;
    let announced = false;
    while (Date.now() - started < VERIFY_TIMEOUT_MS) {
      this.assertOrigin();
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

  async fetch({ path, method = "GET", headers = {}, body }) {
    this.assertOrigin();
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

  async cookies() {
    this.assertOrigin();
    return { cookies: await this.context.cookies(this.origin) };
  }

  async close() {
    this.closing = true;
    if (this.context) {
      await this.context.close().catch(() => {});
      this.context = null;
    }
    return {};
  }
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
  for await (const line of input) {
    if (stopping) break;
    let message;
    try {
      if (Buffer.byteLength(line) > 128 * 1024) throw new WorkerError("浏览器请求过长");
      message = JSON.parse(line);
      if (!["open", "navigate", "verify", "fetch", "cookies", "close"].includes(message.op)) {
        throw new WorkerError("不支持的浏览器操作");
      }
      const data = await worker[message.op](message.params || {});
      emit({ id: message.id, ok: true, data });
    } catch (error) {
      emit({
        id: message?.id,
        ok: false,
        code: error instanceof NeedsHuman ? "needsHuman" : "failed",
        error: error instanceof WorkerError ? error.message
          : String(error).match(/net::ERR_[A-Z_]+/)?.[0]
            ? "浏览器连接失败：" + String(error).match(/net::ERR_[A-Z_]+/)[0]
            : "浏览器操作失败，窗口可能已关闭或站点没有响应",
      });
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
