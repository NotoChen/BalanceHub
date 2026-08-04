import type { TemporaryCliInstance } from "../stores/provider-types";

export type TemporaryCliLaunchPhase = "waiting" | "confirming" | "ready";

interface TemporaryCliLaunchMonitorOptions {
  timeoutMs?: number;
  pollIntervalMs?: number;
  stableForMs?: number;
  now?: () => number;
  wait?: (milliseconds: number) => Promise<void>;
  onProgress?: (percent: number, phase: TemporaryCliLaunchPhase) => void;
}

const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_POLL_INTERVAL_MS = 200;
const DEFAULT_STABLE_FOR_MS = 500;

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => globalThis.setTimeout(resolve, milliseconds));
}

function exitedMessage(instance: TemporaryCliInstance) {
  const detail = instance.exitCode === null ? "未返回退出码" : `退出码 ${instance.exitCode}`;
  return `临时 CLI 在启动完成前已退出（${detail}），请检查终端中的提示`;
}

export async function waitForTemporaryCliStart(
  instanceId: string,
  readInstance: (instanceId: string) => Promise<TemporaryCliInstance | null>,
  options: TemporaryCliLaunchMonitorOptions = {},
): Promise<TemporaryCliInstance> {
  const timeoutMs = Math.max(1, options.timeoutMs ?? DEFAULT_TIMEOUT_MS);
  const pollIntervalMs = Math.max(1, options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS);
  const stableForMs = Math.max(0, options.stableForMs ?? DEFAULT_STABLE_FOR_MS);
  const now = options.now ?? Date.now;
  const wait = options.wait ?? delay;
  const startedAt = now();
  let runningSince: number | null = null;
  let lastReadError = "";

  while (true) {
    const elapsed = Math.max(0, now() - startedAt);
    if (elapsed >= timeoutMs) {
      const seconds = Math.ceil(timeoutMs / 1_000);
      const detail = lastReadError ? `；最后一次状态读取失败：${lastReadError}` : "";
      throw new Error(
        `终端已打开，但 ${seconds} 秒内未收到临时 CLI 启动确认，请检查终端中的提示${detail}`,
      );
    }

    let instance: TemporaryCliInstance | null = null;
    try {
      instance = await readInstance(instanceId);
      lastReadError = "";
    } catch (error) {
      lastReadError = error instanceof Error ? error.message : String(error);
    }

    if (instance?.status === "exited") {
      throw new Error(exitedMessage(instance));
    }
    if (instance?.status === "running") {
      const observedAt = now();
      runningSince ??= observedAt;
      if (observedAt - runningSince >= stableForMs) {
        options.onProgress?.(100, "ready");
        return instance;
      }
      options.onProgress?.(94, "confirming");
    } else if (instance?.status === "starting" || instance === null) {
      runningSince = null;
      const progress = 68 + Math.floor((Math.min(elapsed, timeoutMs) / timeoutMs) * 22);
      options.onProgress?.(Math.min(90, progress), "waiting");
    }

    const remaining = Math.max(1, timeoutMs - Math.max(0, now() - startedAt));
    await wait(Math.min(pollIntervalMs, remaining));
  }
}
