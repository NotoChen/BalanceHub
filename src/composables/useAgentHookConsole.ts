import { computed, onUnmounted, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { useAgentHookStore } from "../stores/agent-hooks";
import { agentHookTargetKey, type AgentHookTargetKey } from "../utils/agent-runtime";
import type {
  AgentCliKind,
  AgentHookMutation,
  AgentHookPlan,
  AgentRuntimeScope,
} from "../stores/provider-types";

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function useAgentHookConsole() {
  const store = useAgentHookStore();
  const rowBusy = ref<Record<string, boolean>>({});
  const applyingTargets = ref<Record<string, boolean>>({});
  const rowErrors = ref<Record<string, string | null>>({});
  const planningTarget = ref<string | null>(null);
  const planVisible = ref(false);
  const pendingPlan = ref<AgentHookPlan | null>(null);
  const disposed = ref(false);
  const rowGeneration = ref<Record<string, number>>({});
  let planRequestId = 0;

  const inspections = computed(() => store.inspections);
  const canApplyPlan = computed(() => Boolean(
    pendingPlan.value?.supported && !pendingPlan.value.conflict && pendingPlan.value.changes.length,
  ));

  function planTargetKey(plan: AgentHookPlan) {
    return agentHookTargetKey(plan.agentKind, plan.runtimeScope);
  }

  function inspectionFor(agentKind: AgentCliKind, scope: AgentRuntimeScope = { kind: "native" }) {
    return store.inspections[agentHookTargetKey(agentKind, scope)] ?? null;
  }

  function isRowBusy(targetKey: AgentHookTargetKey) {
    return Boolean(applyingTargets.value[targetKey] || rowBusy.value[targetKey] || planningTarget.value === targetKey);
  }

  function rowError(targetKey: AgentHookTargetKey) {
    return rowErrors.value[targetKey] ?? store.errors[targetKey] ?? null;
  }

  async function inspect(
    agentKind: AgentCliKind,
    mode: "inspect" | "health" | "verify" = "inspect",
    scope: AgentRuntimeScope = { kind: "native" },
  ) {
    const key = agentHookTargetKey(agentKind, scope);
    // A pre-commit read must not invalidate the apply response or release its row.
    if (applyingTargets.value[key]) return inspectionFor(agentKind, scope);
    const current = (rowGeneration.value[key] ?? 0) + 1;
    rowGeneration.value[key] = current;
    rowErrors.value[key] = null;
    rowBusy.value[key] = true;
    try {
      return await store.inspect(agentKind, mode, scope);
    } catch (error) {
      if (!disposed.value && rowGeneration.value[key] === current) rowErrors.value[key] = errorMessage(error);
      return null;
    } finally {
      if (rowGeneration.value[key] === current) rowBusy.value[key] = false;
    }
  }

  async function requestPlan(
    agentKind: AgentCliKind,
    mutation: AgentHookMutation,
    repair = false,
    scope: AgentRuntimeScope = { kind: "native" },
  ) {
    const key = agentHookTargetKey(agentKind, scope);
    if (planVisible.value || planningTarget.value === key || rowBusy.value[key]) return false;
    const current = ++planRequestId;
    planningTarget.value = key;
    rowErrors.value[key] = null;
    try {
      const plan = repair
        ? await store.repairPlan(agentKind, scope)
        : await store.plan(agentKind, mutation, scope);
      if (disposed.value || current !== planRequestId) return false;
      pendingPlan.value = plan;
      planVisible.value = true;
      return true;
    } catch (error) {
      if (!disposed.value && current === planRequestId) {
        rowErrors.value[key] = errorMessage(error);
        Message.error(errorMessage(error));
      }
      return false;
    } finally {
      if (current === planRequestId) planningTarget.value = null;
    }
  }

  function closePlan() {
    planRequestId += 1;
    planningTarget.value = null;
    planVisible.value = false;
    pendingPlan.value = null;
  }

  async function confirmPlan() {
    const plan = pendingPlan.value;
    if (!plan || !canApplyPlan.value) return;
    const key = planTargetKey(plan);
    if (applyingTargets.value[key]) return;
    const current = (rowGeneration.value[key] ?? 0) + 1;
    rowGeneration.value[key] = current;
    planVisible.value = false;
    pendingPlan.value = null;
    rowBusy.value[key] = true;
    applyingTargets.value[key] = true;
    rowErrors.value[key] = null;
    try {
      await store.apply(plan.agentKind, plan);
      if (!disposed.value && rowGeneration.value[key] === current) Message.success(`${plan.mutation === "remove" ? "删除" : plan.mutation === "disable" ? "停用" : plan.mutation === "enable" ? "启用" : "安装"} Hook 完成`);
    } catch (error) {
      if (!disposed.value && rowGeneration.value[key] === current) {
        rowErrors.value[key] = errorMessage(error);
        Message.error(errorMessage(error));
      }
    } finally {
      if (rowGeneration.value[key] === current) {
        const applyError = rowErrors.value[key];
        applyingTargets.value[key] = false;
        rowBusy.value[key] = false;
        if (!disposed.value) {
          const checking = inspect(plan.agentKind, "health", plan.runtimeScope);
          const checkGeneration = rowGeneration.value[key];
          await checking;
          if (applyError && !disposed.value && rowGeneration.value[key] === checkGeneration) rowErrors.value[key] = applyError;
        }
      }
    }
  }

  function cancel() {
    planRequestId += 1;
    disposed.value = true;
    rowGeneration.value = {};
    rowBusy.value = {};
    applyingTargets.value = {};
    planningTarget.value = null;
    planVisible.value = false;
    pendingPlan.value = null;
  }

  onUnmounted(cancel);

  return {
    inspections,
    rowBusy,
    rowErrors,
    planningTarget,
    planVisible,
    pendingPlan,
    canApplyPlan,
    inspectionFor,
    isRowBusy,
    rowError,
    inspect,
    requestPlan,
    closePlan,
    confirmPlan,
    cancel,
  };
}
