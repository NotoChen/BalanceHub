import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { withTimeout } from "../utils/promise-timeout";

export interface BrowserInfo {
  name: string;
  path: string;
  managed: boolean;
  version: string | null;
}

export type BrowserSelection = { mode: "system"; path: string } | { mode: "managed" };

export interface BrowserRuntimeStatus {
  phase: "notInstalled" | "needsUpdate" | "needsBrowser" | "needsCheck" | "unsupported" | "installing" | "checking" | "ready" | "failed" | "cancelled";
  message: string;
  ready: boolean;
  installed: boolean;
  browser: BrowserInfo | null;
  systemBrowsers: BrowserInfo[];
  selection: BrowserSelection;
  runtimeReady: boolean;
  expectedVersion: string;
  installedVersion: string | null;
  progress: number | null;
  revision: number;
  detectedAt: number;
  canInstall: boolean;
  canUninstall: boolean;
  runtimeDownloadBytes: number;
  browserDownloadBytes: number;
}

export function getBrowserRuntimeStatus(force = false) {
  return withTimeout(invoke<BrowserRuntimeStatus>("get_browser_runtime_status", { force }), 8_000, "检测浏览器组件超时");
}
export function installBrowserRuntime(selection: BrowserSelection, repair = false) {
  return withTimeout(invoke<BrowserRuntimeStatus>("install_browser_runtime", { selection, repair }), 8_000, "提交组件安装超时，请查看后台进度");
}
export function selectBrowserRuntimeBrowser(selection: BrowserSelection) {
  return withTimeout(invoke<BrowserRuntimeStatus>("select_browser_runtime_browser", { selection }), 8_000, "提交浏览器验证超时，请查看后台进度");
}
export function inspectBrowserRuntimeBrowser(path: string) {
  return withTimeout(invoke<BrowserInfo>("inspect_browser_runtime_browser", { path }), 8_000, "读取浏览器程序超时");
}
export function cancelBrowserRuntimeInstall() {
  return withTimeout(invoke<void>("cancel_browser_runtime_install"), 5_000, "取消浏览器操作超时");
}
export function uninstallBrowserRuntime() {
  return withTimeout(invoke<BrowserRuntimeStatus>("uninstall_browser_runtime"), 20_000, "卸载组件超时，请重新检测");
}
export function listenBrowserRuntime(receive: (state: BrowserRuntimeStatus) => void) {
  return listen<BrowserRuntimeStatus>("balancehub://browser-runtime", (event) => receive(event.payload));
}
