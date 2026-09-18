import { withTimeout } from "./promise-timeout.ts";

interface ControlOptions {
  changed: (runId: string, pending: boolean) => void;
  failed: (message: string) => void;
  timeoutMs?: number;
}

export function createLoginTaskControl(options: ControlOptions) {
  const pending = new Set<string>();
  let disposed = false;
  async function run(id: string, action: () => Promise<void>) {
    if (disposed || pending.has(id)) return;
    pending.add(id);
    options.changed(id, true);
    try { await withTimeout(action(), options.timeoutMs ?? 6_500, "操作超时，请查看登录任务状态"); }
    catch (error) { if (!disposed) options.failed(error instanceof Error ? error.message : String(error)); }
    finally {
      pending.delete(id);
      if (!disposed) options.changed(id, false);
    }
  }
  return { run, pending: (id: string) => pending.has(id), dispose: () => { disposed = true; pending.clear(); } };
}
