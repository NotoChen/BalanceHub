import type { ProviderInput } from "../stores/provider-types";
import type { ProviderBrowserLoginTask } from "../api/provider-browser-login";
import { withTimeout } from "./promise-timeout.ts";

interface LoginFlowOptions {
  editorSession: () => number;
  visible: () => boolean;
  input: () => ProviderInput;
  ensureRuntime: () => Promise<boolean>;
  chooseAccount: (input: ProviderInput) => Promise<string | null>;
  cancelSelection: () => void;
  start: (input: ProviderInput, loginAccountId: string) => Promise<ProviderBrowserLoginTask>;
  cancel: (runId: string) => Promise<void>;
  close: () => void;
  pending: (value: boolean) => void;
  started: () => void;
  failed: (message: string) => void;
  startupTimeoutMs?: number;
}

export function createProviderLoginFlow(options: LoginFlowOptions) {
  let revision = 0;
  let pending = false;
  function invalidate() {
    revision++;
    pending = false;
    options.pending(false);
    options.cancelSelection();
  }
  async function run() {
    if (pending || !options.visible()) return;
    const session = options.editorSession();
    const request = ++revision;
    const isCurrent = () => request === revision && session === options.editorSession() && options.visible();
    const input = JSON.parse(JSON.stringify(options.input())) as ProviderInput;
    pending = true;
    options.pending(true);
    try {
      if (!await options.ensureRuntime() || !isCurrent()) return;
      const loginAccountId = await options.chooseAccount(input);
      if (!loginAccountId || !isCurrent()) return;
      const starting = options.start(input, loginAccountId);
      // Closing/reopening the editor or timing out must not leave a late
      // startup running invisibly against an abandoned draft.
      void starting.then((task) => {
        if (!isCurrent()) return options.cancel(task.runId);
      }).catch(() => {});
      await withTimeout(starting, options.startupTimeoutMs ?? 10_000, "创建登录任务超时，请重试");
      if (isCurrent()) {
        options.close();
        options.started();
      }
    } catch (error) {
      if (isCurrent()) options.failed(error instanceof Error ? error.message : String(error));
    } finally {
      if (request === revision) invalidate();
    }
  }
  return { run, invalidate };
}
