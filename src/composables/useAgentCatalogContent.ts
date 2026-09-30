import { onMounted, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import { cancelAgentCatalogRead, readAgentCatalogContent } from "../api/agent-catalog";
import type { AgentCatalogContent } from "../stores/agent-catalog-types";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";
import { withTimeout } from "../utils/promise-timeout";

// This window's recent reads only. Raw native text is never persisted to disk.
const recent = new Map<string, { content: AgentCatalogContent; bytes: number }>();
const cacheLimit = 16 * 1024 * 1024;
let cachedBytes = 0;

function forget(key: string) {
  cachedBytes -= recent.get(key)?.bytes ?? 0;
  recent.delete(key);
}

function remember(key: string, content: AgentCatalogContent) {
  forget(key);
  if (content.pendingSources || content.unavailable.length || !content.groups.length || content.groups.some((group) => !group.complete)) return;
  const bytes = JSON.stringify(content).length * 2;
  if (bytes > cacheLimit) return;
  while (recent.size >= 16 || cachedBytes + bytes > cacheLimit) {
    const oldest = recent.keys().next().value;
    if (oldest === undefined) break;
    forget(oldest);
  }
  recent.set(key, { content, bytes });
  cachedBytes += bytes;
}

export function useAgentCatalogContent(options: {
  assetId: Ref<string | null>;
  workspace: Ref<string | undefined>;
  contentRevision: Ref<string | undefined>;
}) {
  const store = useAgentCatalogStore();
  const content = shallowRef<AgentCatalogContent | null>(null);
  const loading = ref(false);
  const error = ref("");
  let revision = 0;
  let displayedResource = "";
  let active: { key: string; cancel: () => void; task: Promise<void> } | null = null;
  let disposed = false;

  function reload(): Promise<void> {
    const assetId = options.assetId.value;
    const workspace = options.workspace.value ?? null;
    const contentRevision = options.contentRevision.value;
    const resource = JSON.stringify([workspace, assetId]);
    const key = JSON.stringify([workspace, assetId, contentRevision]);
    if (active?.key === key) return active.task;
    const current = ++revision;
    active?.cancel();
    active = null;
    const cached = recent.get(key);
    if (cached) {
      recent.delete(key);
      recent.set(key, cached);
      content.value = cached.content;
    } else if (displayedResource !== resource || !assetId) content.value = null;
    displayedResource = resource;
    error.value = "";
    loading.value = Boolean(assetId);
    if (!assetId || disposed) { loading.value = false; return Promise.resolve(); }
    const requestId = crypto.randomUUID();
    const isCurrent = () => !disposed && current === revision && assetId === options.assetId.value
      && workspace === (options.workspace.value ?? null) && contentRevision === options.contentRevision.value;
    let interrupt: () => void = () => {};
    let finished = false;
    let canceled = false;
    const interrupted = new Promise<never>((_, reject) => { interrupt = () => reject(new Error("资源读取已取消")); });
    const cancel = () => {
      if (finished || canceled) return;
      canceled = true;
      interrupt();
      void cancelAgentCatalogRead(requestId).catch(() => {});
    };
    const task = (async () => {
      try {
        await Promise.race([store.ensureReady(workspace ?? undefined), interrupted]);
        if (!isCurrent() || canceled) return;
        const result = await withTimeout(Promise.race([
          readAgentCatalogContent({ assetId, workspace, requestId }, (partial) => {
            if (isCurrent() && !finished && partial.assetId === assetId) content.value = partial;
          }),
          interrupted,
        ]), 15_000, "读取资源内容超时，请重试");
        finished = true;
        if (!isCurrent()) return;
        if (result.assetId !== assetId) throw new Error("返回的资源不一致，请重新读取");
        content.value = result;
        remember(key, result);
      } catch (failure) {
        cancel();
        if (isCurrent()) { forget(key); error.value = agentEnvironmentErrorMessage(failure); }
      } finally {
        finished = true;
        if (current === revision) { loading.value = false; active = null; }
      }
    })();
    active = { key, cancel, task };
    return task;
  }

  function revalidate() {
    if (!active && options.assetId.value && document.visibilityState !== "hidden") void reload();
  }
  watch([options.assetId, options.workspace, options.contentRevision], reload, { immediate: true });
  onMounted(() => {
    globalThis.addEventListener("focus", revalidate);
    document.addEventListener("visibilitychange", revalidate);
  });
  onScopeDispose(() => {
    disposed = true;
    revision += 1;
    active?.cancel();
    active = null;
    loading.value = false;
    globalThis.removeEventListener?.("focus", revalidate);
    if (typeof document !== "undefined") document.removeEventListener("visibilitychange", revalidate);
  });
  return { content, loading, error, reload };
}
