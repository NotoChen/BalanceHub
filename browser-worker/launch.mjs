import { chromium } from "playwright-core";
import { mkdir, chmod } from "node:fs/promises";

export class WorkerError extends Error {}
export class NeedsHuman extends WorkerError {}

export async function showBrowserWindow(context) {
  const page = context?.pages().filter((item) => !item.isClosed()).at(-1);
  if (!page) throw new WorkerError("登录窗口已关闭，请重新发起登录");
  let timer;
  const show = async () => {
    let cdp;
    try {
      cdp = await context.newCDPSession(page);
      const { windowId } = await cdp.send("Browser.getWindowForTarget");
      await cdp.send("Browser.setWindowBounds", { windowId, bounds: { windowState: "normal" } });
      await page.bringToFront();
      return { shown: true };
    } finally { await cdp?.detach().catch(() => {}); }
  };
  try {
    return await Promise.race([show(), new Promise((_, reject) => {
      timer = setTimeout(() => reject(new WorkerError("显示登录窗口超时，请稍后重试")), 4_000);
    })]);
  } finally { clearTimeout(timer); }
}

export function bootstrapHtml(title, message = "正在准备签到验证…") {
  const escape = (value) => value.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[character]);
  return `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>${escape(title)}</title><body style="font:14px/1.6 system-ui;padding:20px">${escape(message)}</body></html>`;
}

export async function launchBrowser({ profileDir, executablePath, proxy, title, message, width = 420, height = 360, headless = false }, emit) {
  if (!executablePath) throw new WorkerError("没有可用的浏览器，请在 App 中安装组件");
  await mkdir(profileDir, { recursive: true, mode: 0o700 });
  if (process.platform !== "win32") await chmod(profileDir, 0o700);
  const context = await chromium.launchPersistentContext(profileDir, {
    executablePath,
    headless,
    viewport: null,
    timeout: 30_000,
    ignoreDefaultArgs: headless ? [] : ["--enable-automation", "about:blank"],
    args: [
      ...(!headless ? [
        `--app=data:text/html;charset=utf-8,${encodeURIComponent(bootstrapHtml(title, message))}`,
        `--window-size=${width},${height}`,
      ] : []),
      "--test-type",
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
  }).catch((error) => { throw new WorkerError(browserLaunchFailure(error)); });
  try {
    const cdp = await context.browser().newBrowserCDPSession();
    const processes = await cdp.send("SystemInfo.getProcessInfo");
    const browserPid = processes.processInfo.find((item) => item.type === "browser")?.id ?? null;
    await cdp.detach();
    emit({ event: "browserStarted", browserPid });
    return context;
  } catch (error) {
    await context.close().catch(() => {});
    throw error;
  }
}

export function browserLaunchFailure(error) {
  const message = String(error);
  if (/executable.*(?:doesn.t|does not) exist|ENOENT/i.test(message)) return "浏览器程序不存在，请重新选择浏览器";
  if (/EACCES|access.*denied|permission denied/i.test(message)) return "没有执行浏览器的权限，请检查文件权限或系统安全设置";
  if (/missing.*dependencies|shared libraries|lib.*\.so|Host system is missing/i.test(message)) return "浏览器缺少系统依赖，请安装 Chromium 所需依赖或换用本机浏览器";
  if (/display|XServer|X server|Wayland/i.test(message)) return "无法连接图形桌面，请检查桌面会话后重试";
  if (/timeout/i.test(message)) return "浏览器启动超时，请检查系统限制或换用其他浏览器";
  if (/not a valid Win32|Bad CPU|Exec format|incompatible.*architecture/i.test(message)) return "浏览器架构与当前系统不兼容，请选择适合此系统的浏览器";
  return "浏览器启动失败，请检查浏览器能否正常打开，或选择其他 Chromium 浏览器后重新验证";
}
