import { onScopeDispose, ref, watch, type Ref } from "vue";
import { useAgentLifecycleStore } from "../stores/agent-lifecycle";

/** Schedule only when the backend says a release check is due. */
export function useAgentVersionChecks(active: Ref<boolean>, localRevision: Ref<string>) {
  const store = useAgentLifecycleStore();
  const visible = ref(document.visibilityState !== "hidden");
  let timer: ReturnType<typeof globalThis.setTimeout> | null = null;
  let disposed = false;

  function schedule() {
    if (timer !== null) globalThis.clearTimeout(timer);
    timer = null;
    if (disposed || !active.value || !visible.value || store.loading) return;
    const next = store.nextVersionCheckAt;
    if (next === null) return;
    timer = globalThis.setTimeout(() => {
      timer = null;
      if (!disposed && active.value && document.visibilityState !== "hidden") void store.refresh("ifStale");
    }, Math.min(Math.max(next - Date.now(), 250), 2_147_483_647));
  }

  function visibilityChanged() {
    visible.value = document.visibilityState !== "hidden";
    schedule();
  }
  watch(localRevision, () => { store.invalidate(); schedule(); }, { flush: "sync" });
  watch(() => [active.value, visible.value, store.loading, store.nextVersionCheckAt], schedule, { immediate: true });
  globalThis.addEventListener("focus", visibilityChanged);
  document.addEventListener("visibilitychange", visibilityChanged);
  onScopeDispose(() => {
    disposed = true;
    if (timer !== null) globalThis.clearTimeout(timer);
    globalThis.removeEventListener("focus", visibilityChanged);
    document.removeEventListener("visibilitychange", visibilityChanged);
  });
}
