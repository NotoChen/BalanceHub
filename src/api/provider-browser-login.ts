import { invoke } from "@tauri-apps/api/core";
import type { ProviderInput } from "../stores/provider-types";
import { withTimeout } from "../utils/promise-timeout";

export interface ProviderBrowserLoginTask {
  runId: string;
  providerId: string | null;
  providerName: string;
  loginAccountId: string;
  operation: "import" | "account";
  phase: "queued" | "opening" | "waitingLogin" | "saving" | "completed" | "failed" | "cancelled";
  message: string;
  startedAt: number;
  finishedAt: number | null;
  error: string | null;
  canCancel: boolean;
  canShowWindow: boolean;
}

// The editor flow bounds startup and cancels any late acknowledgement.
export const startProviderBrowserLogin = (input: ProviderInput, loginAccountId: string) =>
  invoke<ProviderBrowserLoginTask>("start_provider_browser_login", { input, loginAccountId });

export const listProviderBrowserLogins = () => withTimeout(
  invoke<ProviderBrowserLoginTask[]>("list_provider_browser_logins"), 5_000, "读取登录任务超时",
);

export const cancelProviderBrowserLogin = (runId: string) => withTimeout(
  invoke<void>("cancel_provider_browser_login", { runId }), 5_000, "取消登录超时，请关闭登录窗口",
);

export const showProviderLoginWindow = (runId: string) => withTimeout(
  invoke<void>("show_provider_login_window", { runId }), 6_000, "显示登录窗口超时，请稍后重试",
);
