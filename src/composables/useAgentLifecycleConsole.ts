import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import { useAgentLifecycleStore } from "../stores/agent-lifecycle";
import type { AgentLifecycleActionKind, AgentLifecyclePlan, AgentLifecycleTarget } from "../stores/agent-lifecycle-types";
import type { AgentCliKind } from "../stores/provider-types";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";

export function useAgentLifecycleConsole(navigationRevision: Ref<number>) {
  const store = useAgentLifecycleStore();
  const agentKind = ref<AgentCliKind | null>(null);
  const visible = computed(() => agentKind.value !== null);
  const targets = computed(() => store.catalog?.targets.filter((target) => target.agentKind === agentKind.value) ?? []);
  const plan = shallowRef<AgentLifecyclePlan | null>(null);
  const preparingTargetId = ref<string | null>(null);
  const error = ref("");
  const now = ref(Date.now());
  const expired = computed(() => Boolean(plan.value && (!Number.isFinite(Date.parse(plan.value.expiresAt)) || Date.parse(plan.value.expiresAt) <= now.value)));
  let expectedTarget: { id: string; revision: string } | null = null;
  let request = 0;
  let disposed = false;
  let timer: ReturnType<typeof globalThis.setInterval> | null = null;
  function clearPlan() {
    request += 1;
    plan.value = null;
    expectedTarget = null;
    preparingTargetId.value = null;
    error.value = "";
    if (timer !== null) globalThis.clearInterval(timer);
    timer = null;
  }
  function close() { clearPlan(); agentKind.value = null; }
  function open(kind: AgentCliKind) { close(); agentKind.value = kind; void store.refresh("ifStale"); }
  async function prepare(target: AgentLifecycleTarget, action: AgentLifecycleActionKind) {
    if (!target.actions.some((item) => item.kind === action && item.available)) return;
    clearPlan();
    const current = ++request;
    preparingTargetId.value = target.id;
    expectedTarget = { id: target.id, revision: target.evidenceRevision };
    try {
      const result = await store.plan({ agentKind: target.agentKind, targetId: target.id, action, expectedEvidenceRevision: target.evidenceRevision });
      if (disposed || current !== request) return;
      if (result.targetId !== target.id || result.agentKind !== target.agentKind || result.action !== action) throw new Error("升级计划返回的目标不一致");
      plan.value = result;
      now.value = Date.now();
      timer = globalThis.setInterval(() => { now.value = Date.now(); }, 1_000);
    } catch (failure) {
      if (!disposed && current === request) error.value = agentEnvironmentErrorMessage(failure);
    } finally { if (current === request) preparingTargetId.value = null; }
  }
  function confirm() {
    now.value = Date.now();
    const accepted = plan.value;
    if (!accepted?.planToken || expired.value || preparingTargetId.value) return;
    close();
    void store.apply(accepted);
  }
  watch(navigationRevision, close, { flush: "sync" });
  watch(() => store.catalog, (catalog) => {
    if (!expectedTarget) return;
    const current = catalog?.targets.find((target) => target.id === expectedTarget?.id);
    if (!current || current.evidenceRevision !== expectedTarget.revision) clearPlan();
  }, { flush: "sync" });
  onScopeDispose(() => { disposed = true; close(); });
  return { visible, agentKind, targets, plan, preparingTargetId, error, expired, open, close, prepare, confirm, clearPlan };
}
