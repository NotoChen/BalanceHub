import { defineStore } from "pinia";
import { computed, ref, shallowRef } from "vue";
import {
  getAgentAssetCatalog, getAgentCatalogRevision, hasAgentCatalogChanges, getAgentCatalogDefinition, saveAgentCatalogDefinition, deleteAgentCatalogDefinition,
  adoptAgentCatalogAsset, getAgentCatalogAgentPanel, previewAgentCatalogRelation, commitAgentCatalogRelation, planAgentCatalog, applyAgentCatalog,
  getAgentCatalogOperation, listAgentCatalogOperations, cancelAgentCatalogOperation,
} from "../api/agent-catalog";
import type {
  AgentAssetCatalog, AgentCatalogPlan, AgentCatalogPlanRequest, AgentCatalogSaveRequest,
  AgentCatalogAdoptRequest, AgentCatalogDeleteRequest, AgentCatalogOperation, AgentCatalogAgentPanelRequest,
  AgentCatalogRelationPreviewRequest, AgentCatalogRelationPreview, AgentCatalogRelationMutation,
} from "./agent-catalog-types";
import { agentEnvironmentErrorMessage, agentEnvironmentKey, useAgentEnvironmentStore } from "./agent-environment";
import { withTimeout } from "../utils/promise-timeout";
import { useAgentOperationTracking } from "../composables/useAgentOperationTracking";

interface RelationTask {
  id: string; name: string; action: AgentCatalogRelationMutation["kind"];
  state: "running" | "completed" | "failed" | "unknown"; message: string; startedAt: number; finishedAt?: number;
  workspace?: string;
}

export const useAgentCatalogStore = defineStore("agent-catalog", () => {
  const environment = useAgentEnvironmentStore();
  const catalogs = shallowRef<Record<string, AgentAssetCatalog>>({});
  const loading = ref<Record<string, boolean>>({});
  const loadErrors = ref<Record<string, string>>({});
  const stale = ref<Record<string, boolean>>({});
  const starting = ref<Record<string, boolean>>({});
  const startErrors = ref<Record<string, string>>({});
  const startTimes = ref<Record<string, number>>({});
  const operationNames = ref<Record<string, string>>({});
  const relationTasks = ref<Record<string, RelationTask>>({});
  const relationStarting = new Set<string>();
  let relationSequence = 0;
  const requests = new Map<string, number>();
  const pendingRefreshes = new Map<string, Promise<AgentAssetCatalog | null>>();
  const pendingChecks = new Map<string, Promise<void>>();
  const pendingReadiness = new Map<string, Promise<AgentAssetCatalog>>();
  const dirtyRefreshes = new Set<string>();
  const operationWorkspaces = new Map<string, string | undefined>();
  const submissions = new Map<string, { assetId: string; action: AgentCatalogPlan["action"]; workspace?: string; name: string;
    afterMutation?: (workspace?: string) => Promise<unknown> }>();
  const currentSubmission = new Map<string, string>();
  const knownPlans = new Set<string>();

  function refresh(workspace?: string, afterMutation = false): Promise<AgentAssetCatalog | null> {
    const key = agentEnvironmentKey(workspace);
    const pending = pendingRefreshes.get(key);
    if (pending) { if (afterMutation) dirtyRefreshes.add(key); return pending; }
    const task = (async () => {
      let result: AgentAssetCatalog | null;
      do { dirtyRefreshes.delete(key); result = await refreshNow(workspace); } while (dirtyRefreshes.has(key));
      return result;
    })().finally(() => { if (pendingRefreshes.get(key) === task) pendingRefreshes.delete(key); });
    pendingRefreshes.set(key, task);
    return task;
  }

  async function refreshNow(workspace?: string) {
    const key = agentEnvironmentKey(workspace);
    const request = (requests.get(key) ?? 0) + 1;
    requests.set(key, request);
    const publication = environment.beginInventoryPublication(workspace);
    loading.value[key] = true;
    delete loadErrors.value[key];
    let acceptingSnapshot = true;
    try {
      const result = await withTimeout(getAgentAssetCatalog(workspace ?? null, (snapshot) => {
        if (!acceptingSnapshot || requests.get(key) !== request || catalogs.value[key]) return;
        environment.publishInventory(snapshot.inventory, workspace, publication);
        catalogs.value = { ...catalogs.value, [key]: snapshot };
        stale.value[key] = true;
      }), 30_000, "读取全局资产超时");
      if (requests.get(key) !== request) return null;
      // A newer native inventory may own that store. It must not discard an
      // independently completed catalog publication used by all asset actions.
      environment.publishInventory(result.inventory, workspace, publication);
      catalogs.value = { ...catalogs.value, [key]: result };
      stale.value[key] = false;
      return result;
    } catch (error) {
      if (requests.get(key) === request) {
        stale.value[key] = true;
        loadErrors.value[key] = agentEnvironmentErrorMessage(error);
        environment.rejectInventoryPublication(error, workspace, publication);
      }
      return null;
    } finally {
      acceptingSnapshot = false;
      if (requests.get(key) === request) loading.value[key] = false;
    }
  }

  function ensureReady(workspace?: string): Promise<AgentAssetCatalog> {
    const key = agentEnvironmentKey(workspace);
    const pending = pendingReadiness.get(key);
    if (pending) return pending;
    const task = withTimeout((async () => {
      const refreshing = pendingRefreshes.get(key);
      if (refreshing) {
        const result = await refreshing;
        if (!result) throw new Error(loadErrors.value[key] || "资源目录尚未读取完成，请重试");
        return result;
      }
      const current = catalogs.value[key];
      if (current && !stale.value[key]) {
        const revision = await withTimeout(getAgentCatalogRevision(workspace ?? null), 5_000, "确认资源目录就绪超时");
        // A refresh may have started while the lightweight check was pending.
        const updating = pendingRefreshes.get(key);
        if (updating) {
          const result = await updating;
          if (!result) throw new Error(loadErrors.value[key] || "资源目录更新失败，请重试");
          return result;
        }
        if (revision === current.revision && catalogs.value[key]?.revision === current.revision && !stale.value[key]) return current;
      }
      const result = await refresh(workspace);
      if (!result) throw new Error(loadErrors.value[key] || "资源目录恢复失败，请重试");
      return result;
    })(), 35_000, "等待资源目录就绪超时，请重试").finally(() => {
      if (pendingReadiness.get(key) === task) pendingReadiness.delete(key);
    });
    pendingReadiness.set(key, task);
    return task;
  }

  function invalidate(workspace?: string) {
    const key = agentEnvironmentKey(workspace);
    stale.value[key] = true;
    if (pendingRefreshes.has(key)) dirtyRefreshes.add(key);
  }

  function check(workspace?: string): Promise<void> {
    const key = agentEnvironmentKey(workspace);
    const pending = pendingRefreshes.get(key);
    if (pending) return pending.then(() => undefined);
    const current = catalogs.value[key];
    if (!current) invalidate(workspace);
    if (!current || stale.value[key]) return Promise.resolve();
    const existing = pendingChecks.get(key);
    if (existing) return existing;
    const revision = current.revision;
    const request = requests.get(key);
    const task = (async () => {
      try {
        const changed = await withTimeout(hasAgentCatalogChanges(revision, workspace ?? null), 15_000, "核对资源来源变化超时");
        if (requests.get(key) !== request || catalogs.value[key]?.revision !== revision) return;
        // The view can defer passive refreshes while a preview is open.
        // Explicit actions restore stale metadata through ensureReady.
        if (changed) invalidate(workspace);
        else delete loadErrors.value[key];
      } catch (error) {
        if (requests.get(key) === request) loadErrors.value[key] = agentEnvironmentErrorMessage(error);
      }
    })().finally(() => { if (pendingChecks.get(key) === task) pendingChecks.delete(key); });
    pendingChecks.set(key, task);
    return task;
  }

  const tracking = useAgentOperationTracking<AgentCatalogOperation>({
    isSettled: (operation) => operation.phase === "completed",
    get: getAgentCatalogOperation, list: listAgentCatalogOperations, cancel: cancelAgentCatalogOperation,
    identity: (operation) => JSON.stringify([operation.planId, operation.assetId, operation.action]),
    timeoutMs: 180_000,
    accepted(operation) {
      const submission = submissions.get(operation.planId);
      if (!submission) return;
      if (submission.assetId !== operation.assetId || submission.action !== operation.action) throw new Error("后台任务与本次提交不一致，请刷新状态");
      knownPlans.add(operation.planId);
      operationWorkspaces.set(operation.id, submission.workspace);
      operationNames.value[operation.id] = submission.name;
      if (currentSubmission.get(operation.assetId) === operation.planId) {
        starting.value[operation.assetId] = false;
        delete startErrors.value[operation.assetId];
        delete startTimes.value[operation.assetId];
        currentSubmission.delete(operation.assetId);
      }
    },
    completed(operation) {
      const submission = submissions.get(operation.planId);
      if (submission?.afterMutation) {
        void submission.afterMutation(submission.workspace);
        return;
      }
      const known = operationWorkspaces.has(operation.id);
      if (known) invalidate(operationWorkspaces.get(operation.id));
      else for (const [key, catalog] of Object.entries(catalogs.value)) {
        if (catalog.assets.some((asset) => asset.id === operation.assetId)) invalidate(key === "__native__" ? undefined : key);
      }
    },
  });
  const nativeOperationIds = computed(() => new Set(Object.values(tracking.operations.value).flatMap((operation) =>
    operation.targets.flatMap((target) => target.nativeOperationId ? [target.nativeOperationId] : []),
  )));

  async function apply(plan: AgentCatalogPlan, workspace?: string, afterMutation?: (workspace?: string) => Promise<unknown>) {
    if (!plan.token || !plan.planId || starting.value[plan.assetId]) return;
    const planId = plan.planId;
    const name = plan.definitionChange?.name || catalogs.value[agentEnvironmentKey(workspace)]?.assets.find((asset) => asset.id === plan.assetId)?.name || "资源";
    submissions.set(planId, { assetId: plan.assetId, action: plan.action, workspace, name, afterMutation });
    currentSubmission.set(plan.assetId, planId);
    starting.value[plan.assetId] = true;
    startTimes.value[plan.assetId] = Date.now();
    delete startErrors.value[plan.assetId];
    try {
      const operation = await withTimeout(applyAgentCatalog({ planToken: plan.token, assetId: plan.assetId, action: plan.action }), 15_000,
        "未能确认后台操作是否开始，请刷新任务状态；不会自动重试");
      if (operation.planId !== planId || operation.assetId !== plan.assetId || operation.action !== plan.action) throw new Error("后台操作返回的计划、资产或动作不一致");
      tracking.track(operation);
    } catch (error) {
      if (currentSubmission.get(plan.assetId) === planId && !knownPlans.has(planId)) startErrors.value[plan.assetId] = agentEnvironmentErrorMessage(error);
      void tracking.recover();
    } finally {
      if (currentSubmission.get(plan.assetId) === planId) starting.value[plan.assetId] = false;
    }
  }

  async function commitRelation(preview: AgentCatalogRelationPreview, workspace?: string) {
    if (!preview.token || !preview.available || preview.action === "compare" || relationStarting.has(preview.relationKey)) return false;
    const id = `catalog-relation-${++relationSequence}`;
    const task: RelationTask = { id, name: preview.sides.map((side) => side.name).filter((name, index, all) => all.indexOf(name) === index).join(" / ") || "资源对应关系",
      action: preview.action, state: "running", message: "正在整理资源对应关系", startedAt: Date.now(), workspace };
    relationTasks.value[id] = task;
    relationStarting.add(preview.relationKey);
    const timeoutMessage = "整理结果尚未确认，请刷新目录核对；不会自动重试";
    try {
      const result = await withTimeout(commitAgentCatalogRelation({ planToken: preview.token, relationKey: preview.relationKey, action: preview.action }), 30_000, timeoutMessage);
      relationTasks.value[id] = { ...task, state: "completed", message: result.message, finishedAt: Date.now() };
    } catch (error) {
      const message = agentEnvironmentErrorMessage(error);
      relationTasks.value[id] = { ...task, state: message === timeoutMessage ? "unknown" : "failed", message, finishedAt: Date.now() };
    } finally {
      relationStarting.delete(preview.relationKey);
    }
    return true;
  }

  return {
    catalogs, loading, loadErrors, stale, invalidate, check, ensureReady, starting, startErrors, startTimes, operationNames, relationTasks, nativeOperationIds, refresh, apply, commitRelation, ...tracking,
    plan: (request: AgentCatalogPlanRequest, requestId: string) => withTimeout(planAgentCatalog(request, requestId), 15_000, "读取配置或生成预览超时，请重试"),
    definition: (id: string) => withTimeout(getAgentCatalogDefinition(id), 15_000, "读取共享定义超时"),
    save: (request: AgentCatalogSaveRequest) => withTimeout(saveAgentCatalogDefinition(request), 30_000, "保存共享定义超时"),
    deleteDefinition: (request: AgentCatalogDeleteRequest) => withTimeout(deleteAgentCatalogDefinition(request), 30_000, "删除结果尚未确认，请刷新共享库核对"),
    adopt: (request: AgentCatalogAdoptRequest) => withTimeout(adoptAgentCatalogAsset(request), 30_000, "收录资产超时"),
    agentPanel: (request: AgentCatalogAgentPanelRequest, requestId: string) => withTimeout(getAgentCatalogAgentPanel(request, requestId), 15_000, "读取配置状态超时，请重试"),
    previewRelation: (request: AgentCatalogRelationPreviewRequest) => withTimeout(previewAgentCatalogRelation(request), 30_000, "比较资源超时，请重新读取"),
    refreshRelation: (id: string) => relationTasks.value[id] ? refresh(relationTasks.value[id].workspace) : Promise.resolve(null),
  };
});
