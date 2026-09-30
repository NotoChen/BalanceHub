import { onScopeDispose, ref } from "vue";
import { withTimeout } from "../utils/promise-timeout.ts";

/** Only the latest, still-open request may publish. Invalidation does not cancel the underlying IPC. */
export function useLatestRequest(options: { timeoutMessage: string; timeoutMs?: number }) {
  const loading = ref(false);
  const error = ref("");
  let revision = 0;
  let disposed = false;

  function invalidate() {
    revision += 1;
    loading.value = false;
    error.value = "";
  }

  async function run<T>(operation: () => Promise<T>, publish: (result: T) => void) {
    if (disposed) return;
    const request = ++revision;
    loading.value = true;
    error.value = "";
    try {
      const result = await withTimeout(
        Promise.resolve().then(operation),
        options.timeoutMs ?? 45_000,
        options.timeoutMessage,
      );
      if (!disposed && request === revision) publish(result);
    } catch (failure) {
      if (!disposed && request === revision) {
        error.value = failure instanceof Error ? failure.message : String(failure);
      }
    } finally {
      if (request === revision) loading.value = false;
    }
  }

  onScopeDispose(() => {
    disposed = true;
    invalidate();
  });

  return { loading, error, run, invalidate };
}
