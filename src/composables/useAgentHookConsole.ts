import { computed, onUnmounted, ref, type Ref } from "vue";
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

export function useAgentHookConsole(agentKinds: Ref<AgentCliKind[]>) {
  const store = useAgentHookStore();
  const rowBusy = ref<Record<string, boolean>>({});
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
    return Boolean(rowBusy.value[targetKey] || planningTarget.value === targetKey);
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

  async function inspectAll() {
    const unique = [...new Set(agentKinds.value)];
    let cursor = 0;
    const worker = async () => {
      while (!disposed.value) {
        const index = cursor++;
        if (index >= unique.length) return;
        await inspect(unique[index]);
      }
    };
    await Promise.all(Array.from({ length: Math.min(3, unique.length) }, () => worker()));
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
    planVisible.value = false;
    pendingPlan.value = null;
  }

  async function confirmPlan() {
    const plan = pendingPlan.value;
    if (!plan || !canApplyPlan.value) return;
    const key = planTargetKey(plan);
    const current = (rowGeneration.value[key] ?? 0) + 1;
    rowGeneration.value[key] = current;
    planVisible.value = false;
    pendingPlan.value = null;
    rowBusy.value[key] = true;
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
        rowBusy.value[key] = false;
        if (!disposed.value) await inspect(plan.agentKind, "health", plan.runtimeScope);
      }
    }
  }

  function cancel() {
    planRequestId += 1;
    disposed.value = true;
    rowGeneration.value = {};
    rowBusy.value = {};
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
    inspectAll,
    requestPlan,
    closePlan,
    confirmPlan,
    cancel,
  };
}
