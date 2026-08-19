import type { TemporaryCliInstance } from "../stores/provider-types";

interface TemporaryCliLaunchMonitorOptions {
  timeoutMs?: number;
  pollIntervalMs?: number;
  stableForMs?: number;
  readTimeoutMs?: number;
  now?: () => number;
  wait?: (milliseconds: number) => Promise<void>;
}

const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_POLL_INTERVAL_MS = 200;
const DEFAULT_STABLE_FOR_MS = 500;
const DEFAULT_READ_TIMEOUT_MS = 3_000;

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
  const readTimeoutMs = Math.max(1, options.readTimeoutMs ?? DEFAULT_READ_TIMEOUT_MS);
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
      const remaining = Math.max(1, timeoutMs - elapsed);
      instance = await readWithTimeout(
        () => readInstance(instanceId),
        Math.min(readTimeoutMs, remaining),
      );
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
        return instance;
      }
    } else if (instance?.status === "starting" || instance === null) {
      runningSince = null;
    }

    const remaining = Math.max(1, timeoutMs - Math.max(0, now() - startedAt));
    await wait(Math.min(pollIntervalMs, remaining));
  }
}

async function readWithTimeout<T>(read: () => Promise<T>, timeoutMs: number): Promise<T> {
  let timeoutId: ReturnType<typeof globalThis.setTimeout> | null = null;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = globalThis.setTimeout(
      () => reject(new Error(`状态读取超过 ${Math.ceil(timeoutMs / 1_000)} 秒`)),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([read(), timeout]);
  } finally {
    if (timeoutId !== null) {
      globalThis.clearTimeout(timeoutId);
    }
  }
}
