import { withTimeout } from "./promise-timeout.ts";

export interface SecretView { value: string; revealed: boolean; pending: boolean; copied: boolean; error: string }

// View-local secrets never survive a close, account switch, hide, or late IPC result.
export function createSecretController(read: () => Promise<string>, copy: (value: string) => Promise<void>, changed: (state: SecretView) => void, timeoutMs = 10_000) {
  let revision = 0;
  let state: SecretView = { value: "", revealed: false, pending: false, copied: false, error: "" };
  const emit = () => changed({ ...state });
  function reset() { revision++; state = { value: "", revealed: false, pending: false, copied: false, error: "" }; emit(); }
  async function run(action: "reveal" | "copy") {
    if (state.pending) return;
    if (action === "reveal" && state.revealed) { reset(); return; }
    const request = ++revision;
    state.pending = true; state.error = ""; state.copied = false; emit();
    try {
      const value = await withTimeout(read(), timeoutMs, "读取凭据超时，请重试");
      if (request !== revision) return;
      if (action === "copy") {
        await withTimeout(copy(value), timeoutMs, "复制超时，请重试");
        if (request === revision) state.copied = true;
      } else { state.value = value; state.revealed = true; }
    } catch (e) {
      if (request === revision) state.error = e instanceof Error ? e.message : String(e);
    } finally {
      if (request === revision) { state.pending = false; emit(); }
    }
  }
  return { reset, reveal: () => run("reveal"), copy: () => run("copy") };
}
