import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import { cancelAgentSessionQuery, countAgentSessions } from "../api/agent-sessions";
import { withTimeout } from "../utils/promise-timeout";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentSessionCount } from "../stores/agent-session-types";

export function useAgentHistorySummary(active: Ref<boolean>, scopeKey: Ref<string>, kinds: Ref<AgentCliKind[]>) {
  // Rust retains request generations across frontend reloads in the same window.
  const consumerId = `agent-history-count-${crypto.randomUUID()}`;
  const agentCounts = shallowRef<AgentSessionCount[]>([]);
  const errors = ref<Partial<Record<AgentCliKind, string>>>({});
  const countingKinds = ref<AgentCliKind[]>([]);
  const busy = computed(() => countingKinds.value.length > 0);
  let revision = 0;
  let disposed = false;
  let lastRead = 0;
  let pending: { requestId: number; kinds: AgentCliKind[]; task: Promise<void> } | null = null;

  function cancelRequest(requestId: number) {
    void withTimeout(cancelAgentSessionQuery({ consumerId, requestId }), 5_000, "取消会话统计超时").catch(() => undefined);
  }
  function cancel() {
    revision += 1;
    if (pending) cancelRequest(pending.requestId);
    pending = null;
    countingKinds.value = [];
  }
  function visible() { return typeof document === "undefined" || document.visibilityState !== "hidden"; }

  function refresh(kind?: AgentCliKind): Promise<void> {
    if (disposed || !active.value || !visible()) return Promise.resolve();
    let selected = kind ? [kind] : [...kinds.value];
    if (!selected.length) return Promise.resolve();
    if (pending && selected.every((item) => pending!.kinds.includes(item))) return pending.task;
    // Preserve the other requested Agents when replacing an unfinished read.
    if (pending) selected = [...new Set([...pending.kinds, ...selected])];
    cancel();
    const requestId = ++revision;
    const scope = scopeKey.value;
    countingKinds.value = selected;
    for (const item of selected) delete errors.value[item];
    const current = () => !disposed && requestId === revision && scope === scopeKey.value && active.value;
    const task = (async () => {
      try {
        const result = await withTimeout(countAgentSessions({ consumerId, requestId, agentKinds: selected }), 25_000, "读取会话数量超时");
        if (!current()) return;
        const next = new Map(agentCounts.value.map((count) => [count.agentKind, count]));
        for (const item of selected) {
          const count = result.counts.find((entry) => entry.agentKind === item);
          if (count) next.set(item, count);
          else next.delete(item);
          if (count?.total != null) delete errors.value[item];
          else errors.value[item] = result.errors[item] || "未能完整读取原生会话索引";
        }
        agentCounts.value = [...next.values()];
        lastRead = Date.now();
      } catch (failure) {
        if (!current()) return;
        cancelRequest(requestId);
        const message = failure instanceof Error ? failure.message : String(failure);
        for (const item of selected) errors.value[item] = message;
      } finally {
        if (requestId === revision) countingKinds.value = [];
        if (pending?.requestId === requestId) pending = null;
      }
    })();
    pending = { requestId, kinds: selected, task };
    return task;
  }

  watch([active, scopeKey, () => kinds.value.join("|")], ([enabled, scope, agents], previous) => {
    if (!previous || scope !== previous[1] || agents !== previous[2]) {
      cancel(); agentCounts.value = []; errors.value = {}; lastRead = 0;
    }
    if (enabled) void refresh();
    else cancel();
  }, { immediate: true });
  function revisit() {
    if (!visible()) { cancel(); return; }
    if (active.value && Date.now() - lastRead >= 30_000) void refresh();
  }
  globalThis.addEventListener?.("focus", revisit);
  if (typeof document !== "undefined") document.addEventListener("visibilitychange", revisit);
  onScopeDispose(() => {
    disposed = true; cancel();
    globalThis.removeEventListener?.("focus", revisit);
    if (typeof document !== "undefined") document.removeEventListener("visibilitychange", revisit);
  });
  return { agentCounts, busy, refresh,
    isLoading: (kind: AgentCliKind) => countingKinds.value.includes(kind),
    errorFor: (kind: AgentCliKind) => errors.value[kind] ?? "",
  };
}
