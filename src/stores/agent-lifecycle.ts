import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";
import {
  getAgentLifecycleCatalog, planAgentLifecycle, applyAgentLifecycle,
  getAgentLifecycleOperation, listAgentLifecycleOperations, cancelAgentLifecycleOperation,
  setAgentAutomaticUpdate,
} from "../api/agent-lifecycle";
import type { AgentAutomaticUpdate, AgentLifecycleCatalog, AgentLifecycleOperation, AgentLifecyclePlan, AgentLifecyclePlanRequest, AgentLifecycleTarget, AgentLifecycleVersionRefresh } from "./agent-lifecycle-types";
import { agentEnvironmentErrorMessage } from "./agent-environment";
import { useAgentCatalogStore } from "./agent-catalog";
import { useAgentOverviewStore } from "./agent-overview";
import { useAgentWorkspaceStore } from "./agent-workspace";
import { useCliRuntimeStore } from "./cli-runtime";
import { withTimeout } from "../utils/promise-timeout";
import { useAgentOperationTracking } from "../composables/useAgentOperationTracking";

export const useAgentLifecycleStore = defineStore("agent-lifecycle", () => {
  const catalog = shallowRef<AgentLifecycleCatalog | null>(null);
  const loading = ref(false);
  const checkingVersions = ref(false);
  const error = ref("");
  const eventError = ref("");
  const starting = ref<Record<string, boolean>>({});
  const startErrors = ref<Record<string, string>>({});
  const startTimes = ref<Record<string, number>>({});
  const automaticSaving = ref<Record<string, boolean>>({});
  const automaticErrors = ref<Record<string, string>>({});
  let catalogRevision = 0;
  const assets = useAgentCatalogStore();
  const requestRevision = ref(0);
  let inputRevision = 0;
  let pendingInputRevision = 0;
  let pendingMode: AgentLifecycleVersionRefresh = "cached";
  let pendingRefresh: Promise<AgentLifecycleCatalog | null> | null = null;
  const refreshPriority: Record<AgentLifecycleVersionRefresh, number> = { cached: 0, ifStale: 1, force: 2 };

  function acceptCatalog(value: AgentLifecycleCatalog) {
    if (value.revision <= catalogRevision) return;
    catalogRevision = value.revision;
    catalog.value = value;
    error.value = "";
  }

  async function setAutomatic(entry: AgentAutomaticUpdate, enabled: boolean, target?: AgentLifecycleTarget) {
    const key = entry.key;
    if (!key || automaticSaving.value[key]) return;
    automaticSaving.value[key] = true;
    delete automaticErrors.value[key];
    try {
      const result = await withTimeout(setAgentAutomaticUpdate({ key, enabled, targetId: target?.id ?? null,
        expectedEvidenceRevision: target?.evidenceRevision ?? null }), 70_000, "保存自动更新设置超时，请刷新确认结果");
      acceptCatalog(result);
    } catch (failure) {
      automaticErrors.value[key] = agentEnvironmentErrorMessage(failure);
      void refresh("cached");
    } finally {
      automaticSaving.value[key] = false;
    }
  }

  function invalidate() {
    inputRevision += 1;
    catalog.value = null;
    error.value = "";
  }

  function refresh(mode: AgentLifecycleVersionRefresh = "cached"): Promise<AgentLifecycleCatalog | null> {
    if (pendingRefresh) {
      return pendingInputRevision !== inputRevision || refreshPriority[mode] > refreshPriority[pendingMode]
        ? pendingRefresh.then(() => refresh(mode)) : pendingRefresh;
    }
    pendingInputRevision = inputRevision;
    pendingMode = mode;
    const task = refreshNow(mode, inputRevision).finally(() => { if (pendingRefresh === task) pendingRefresh = null; });
    pendingRefresh = task;
    return task;
  }

  async function refreshNow(mode: AgentLifecycleVersionRefresh, inputs: number) {
    const request = ++requestRevision.value;
    loading.value = true;
    checkingVersions.value = mode !== "cached";
    error.value = "";
    try {
      if (!catalog.value && mode !== "cached") {
        const cached = await withTimeout(getAgentLifecycleCatalog({ versionRefresh: "cached" }), 65_000, "读取安装缓存超时");
        if (request !== requestRevision.value || inputs !== inputRevision) return null;
        acceptCatalog(cached);
      }
      const result = await withTimeout(getAgentLifecycleCatalog({ versionRefresh: mode }), 65_000, "读取安装与版本信息超时");
      if (request !== requestRevision.value || inputs !== inputRevision) return null;
      acceptCatalog(result);
      return result;
    } catch (failure) {
      if (request === requestRevision.value && inputs === inputRevision) {
        error.value = agentEnvironmentErrorMessage(failure);
      }
      return null;
    } finally {
      if (request === requestRevision.value) { loading.value = false; checkingVersions.value = false; }
    }
  }

  const completedTasks = new Set<string>();
  const tracking = useAgentOperationTracking<AgentLifecycleOperation>({
    isSettled: (operation) => operation.phase === "completed",
    get: getAgentLifecycleOperation, list: listAgentLifecycleOperations, cancel: cancelAgentLifecycleOperation,
    identity: (operation) => JSON.stringify([operation.agentKind, operation.targetId, operation.action]),
    timeoutMs: 20 * 60_000,
    completed(operation) {
      if (operation.recovered || completedTasks.has(operation.id)) return;
      completedTasks.add(operation.id);
      if (operation.origin !== "automatic") {
        const hadCatalog = Boolean(catalog.value);
        invalidate();
        if (hadCatalog) void refresh("ifStale");
      }
      void useAgentOverviewStore().invalidate(operation.agentKind);
      void useCliRuntimeStore().probeCliTools(false).catch(() => undefined);
      for (const [key, snapshot] of Object.entries(assets.catalogs)) {
        if (snapshot.targets.some((target) => target.agentKind === operation.agentKind)) assets.invalidate(key === "__native__" ? undefined : key);
      }
    },
  });

  async function apply(plan: AgentLifecyclePlan) {
    if (starting.value[plan.targetId]) return;
    starting.value[plan.targetId] = true;
    startTimes.value[plan.targetId] = Date.now();
    delete startErrors.value[plan.targetId];
    try {
      const operation = await withTimeout(applyAgentLifecycle({ planToken: plan.planToken, agentKind: plan.agentKind, targetId: plan.targetId, action: plan.action }), 15_000,
        "未能确认升级任务是否已开始，请刷新任务状态；不会自动重试");
      if (operation.targetId !== plan.targetId || operation.agentKind !== plan.agentKind || operation.action !== plan.action) throw new Error("升级任务返回的目标不一致");
      tracking.track(operation);
      useAgentWorkspaceStore().openOperation("lifecycle", operation.id);
    } catch (failure) {
      startErrors.value[plan.targetId] = agentEnvironmentErrorMessage(failure);
      void tracking.recover();
    } finally {
      starting.value[plan.targetId] = false;
    }
  }

  return {
    catalog, loading, checkingVersions, error, eventError, requestRevision, starting, startErrors, startTimes, refresh, invalidate, apply, ...tracking,
    acceptCatalog, automaticSaving, automaticErrors, setAutomatic,
    plan: (request: AgentLifecyclePlanRequest) => withTimeout(planAgentLifecycle(request), 65_000, "生成升级计划超时"),
  };
});
