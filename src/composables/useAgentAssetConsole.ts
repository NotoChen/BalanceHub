import { computed, onUnmounted, ref, watch, type Ref } from "vue";
import type {
  AgentAssetAccessRisk,
  AgentAssetActionKind,
  AgentAssetOperationOutcome,
  AgentAssetOperationPhase,
  AgentAssetPlan,
  AgentAssetReadResult,
  AgentEnvironmentInventory,
} from "../stores/provider-types";
import {
  agentAssetTargetKey,
  agentEnvironmentErrorMessage,
  agentEnvironmentKey,
  useAgentEnvironmentStore,
} from "../stores/agent-environment";
import { agentAssetActionReason, useAgentAssetCatalog } from "./useAgentAssetCatalog";
import { useClipboard } from "./useClipboard";
import { withTimeout } from "../utils/promise-timeout";
import { cancelAgentCatalogRead } from "../api/agent-catalog";
import { useAgentCatalogStore } from "../stores/agent-catalog";

export type AgentAssetDetailTarget = "asset" | "source";
export type AgentAssetPreviewState = "idle" | "loading" | "ready" | "error";
export interface AgentAssetAccessConfirmation {
  kind: AgentAssetDetailTarget;
  id: string;
  action: "open" | "reveal";
  accessId: string;
  environmentId: string;
  workspaceKey: string;
  generation: number;
  label: string;
  path: string | null;
  risks: AgentAssetAccessRisk[];
}
export const agentAssetOperationPhaseLabels: Record<AgentAssetOperationPhase, string> = {
  preparing: "准备中", waitingForLock: "等待同源操作", revalidating: "重新校验", applying: "正在应用", verifying: "验证结果", completed: "已完成",
};
export const agentAssetOperationOutcomeLabels: Record<AgentAssetOperationOutcome, string> = {
  canceledBeforeCommit: "提交前已取消", unchangedConflict: "配置冲突，未修改", unchangedFailure: "操作失败，未修改",
  appliedVerified: "已应用并验证", appliedUnverified: "已应用，尚未验证", outcomeUnknown: "结果未确认",
};
export const agentAssetAccessRiskLabels: Record<AgentAssetAccessRisk, string> = {
  externalPathnameRace: "系统应用会按路径打开文件；文件路径在交给外部应用后仍可能被其他进程替换。",
  rawSensitiveContent: "外部应用将读取文件原始内容，其中可能包含密钥、令牌、Cookie 或密码。",
};

export function useAgentAssetConsole(options: {
  inventory: Ref<AgentEnvironmentInventory | null>;
  workspace: Ref<string | undefined>;
}) {
  const store = useAgentEnvironmentStore();
  const catalogStore = useAgentCatalogStore();
  const clipboard = useClipboard();
  const catalog = useAgentAssetCatalog(options.inventory);
  const selectedAssetId = ref<string | null>(null);
  const selectedSourceId = ref<string | null>(null);
  const preview = ref<AgentAssetReadResult | null>(null);
  const previewState = ref<AgentAssetPreviewState>("idle");
  const previewError = ref("");
  const copiedPathId = ref<string | null>(null);
  const accessConfirmation = ref<AgentAssetAccessConfirmation | null>(null);
  const planVisible = ref(false);
  const pendingPlan = ref<AgentAssetPlan | null>(null);
  const planError = ref("");
  const planningAssetId = ref<string | null>(null);
  const actionBusy = ref<Record<string, boolean>>({});
  const actionErrors = ref<Record<string, string | null>>({});
  const now = ref(Date.now());
  let generation = 0;
  let detailGeneration = 0;
  let planRequestId = 0;
  let activePlanRead: string | null = null;
  function cancelPlanRead() {
    if (activePlanRead) void cancelAgentCatalogRead(activePlanRead).catch(() => {});
    activePlanRead = null;
  }
  let actionRequestId = 0;
  let planWorkspace: string | undefined;
  let planIntent: { id: string; action: AgentAssetActionKind } | null = null;
  const actionRequests = new Map<string, number>();
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;
  let expirationTimer: ReturnType<typeof setInterval> | null = null;
  let disposed = false;

  const workspaceKey = computed(() => agentEnvironmentKey(options.workspace.value));
  const selectedDetail = computed(() => selectedAssetId.value ? catalog.detailFor(selectedAssetId.value) : null);
  const selectedSource = computed(() => selectedSourceId.value ? catalog.indexes.value.sources.get(selectedSourceId.value) ?? null : null);
  const drawerVisible = computed(() => Boolean(selectedDetail.value || selectedSource.value));
  const planExpired = computed(() => Boolean(pendingPlan.value && Date.parse(pendingPlan.value.expiresAt) <= now.value));
  const canApplyPlan = computed(() => Boolean(pendingPlan.value?.token && !planExpired.value && !planningAssetId.value));
  const operations = computed(() => Object.values(store.operations).sort((left, right) => right.createdAt.localeCompare(left.createdAt)));

  function target(kind: AgentAssetDetailTarget, id: string) {
    return kind === "asset" ? catalog.indexes.value.assets.get(id) : catalog.indexes.value.sources.get(id);
  }

  function rowKey(id: string) { return agentAssetTargetKey(id, options.workspace.value); }
  function operationFor(id: string) {
    const operationId = store.operationIdsByTarget[rowKey(id)];
    return operationId ? store.operations[operationId] ?? null : null;
  }
  function rowBusy(id: string) { return Boolean(actionBusy.value[rowKey(id)] || store.operationBusy[rowKey(id)] || planningAssetId.value === id); }
  function rowError(id: string) { return actionErrors.value[rowKey(id)] ?? store.operationErrors[rowKey(id)] ?? null; }
  function rowProgress(id: string) {
    if (planningAssetId.value === id) return "准备变更计划";
    if (actionBusy.value[rowKey(id)]) return "读取或打开中";
    const operation = operationFor(id);
    return operation ? agentAssetOperationPhaseLabels[operation.phase] : null;
  }
  function rowCanCancel(id: string) { return Boolean(operationFor(id)?.canCancel); }
  function cancelRow(id: string) { const operation = operationFor(id); if (operation) void store.cancelOperation(operation.id); }

  function clearPreview() {
    detailGeneration += 1;
    preview.value = null;
    previewState.value = "idle";
    previewError.value = "";
  }
  function closeDetail() {
    clearPreview();
    selectedAssetId.value = null;
    selectedSourceId.value = null;
    accessConfirmation.value = null;
  }
  function openDetail(id: string) {
    if (!catalog.indexes.value.assets.get(id)?.actions.some((action) => action.action === "inspect" && action.available)) return;
    clearPreview();
    selectedAssetId.value = id;
    selectedSourceId.value = null;
  }
  function openSourceDetail(id: string) {
    if (!catalog.indexes.value.sources.get(id)?.actions.some((action) => action.action === "inspect" && action.available)) return;
    clearPreview();
    selectedAssetId.value = null;
    selectedSourceId.value = id;
  }

  async function readPreview(kind: AgentAssetDetailTarget, id: string) {
    const current = target(kind, id);
    if (!current || current.access.kind !== "ready" || !current.actions.some((action) => action.action === "preview" && action.available)) return;
    if (kind === "asset" && selectedAssetId.value !== id) openDetail(id);
    if (kind === "source" && selectedSourceId.value !== id) openSourceDetail(id);
    const request = ++detailGeneration;
    const workspace = options.workspace.value;
    const capturedGeneration = generation;
    const key = agentAssetTargetKey(id, workspace);
    const actionRequest = ++actionRequestId;
    actionRequests.set(key, actionRequest);
    actionBusy.value[key] = true;
    actionErrors.value[key] = null;
    preview.value = null;
    previewState.value = "loading";
    previewError.value = "";
    try {
      const result = await store.readConfigPreview(id, {
        accessId: current.access.accessId,
        environmentId: current.environmentId,
        workspace: options.inventory.value?.workspace ?? undefined,
      }, kind);
      if (disposed || request !== detailGeneration || workspace !== options.workspace.value) return;
      if (capturedGeneration !== generation) throw new Error("目录在读取期间发生变化，请重新读取当前配置");
      preview.value = result;
      previewState.value = "ready";
    } catch (error) {
      if (disposed || request !== detailGeneration || workspace !== options.workspace.value) return;
      previewError.value = capturedGeneration !== generation
        ? "目录在读取期间发生变化，请重新读取当前配置"
        : agentEnvironmentErrorMessage(error);
      previewState.value = "error";
    } finally {
      if (actionRequests.get(key) === actionRequest) actionBusy.value[key] = false;
      if (!disposed && request === detailGeneration && previewState.value === "loading") previewState.value = "idle";
    }
  }

  function closeAccessConfirmation() { accessConfirmation.value = null; }
  async function executeOpen(confirmation: AgentAssetAccessConfirmation, acceptedRisks: AgentAssetAccessRisk[]) {
    if (confirmation.generation !== generation || confirmation.workspaceKey !== workspaceKey.value || disposed) return;
    const current = target(confirmation.kind, confirmation.id);
    const currentAction = current?.actions.find((action) => action.action === confirmation.action && action.available);
    if (!current || !currentAction || current.access.kind !== "ready" || current.access.accessId !== confirmation.accessId) return;
    const key = rowKey(confirmation.id);
    const request = ++actionRequestId;
    const capturedGeneration = generation;
    actionRequests.set(key, request);
    actionBusy.value[key] = true;
    actionErrors.value[key] = null;
    try {
      const access = {
        accessId: confirmation.accessId,
        environmentId: confirmation.environmentId,
        workspace: options.inventory.value?.workspace ?? undefined,
      };
      const openTarget = confirmation.action === "reveal" ? "reveal" : "asset";
      await (confirmation.kind === "asset"
        ? store.openAsset(confirmation.id, access, openTarget, acceptedRisks)
        : store.openSource(confirmation.id, access, openTarget, acceptedRisks));
    } catch (error) {
      if (!disposed && capturedGeneration === generation && actionRequests.get(key) === request) actionErrors.value[key] = agentEnvironmentErrorMessage(error);
    } finally {
      if (actionRequests.get(key) === request) actionBusy.value[key] = false;
    }
  }
  function confirmAccess() {
    const confirmation = accessConfirmation.value;
    accessConfirmation.value = null;
    if (confirmation) void executeOpen(confirmation, confirmation.risks);
  }

  async function requestPlan(id: string, requestedAction: AgentAssetActionKind) {
    if (rowBusy(id)) return;
    closePlan();
    const request = ++planRequestId;
    const requestId = crypto.randomUUID();
    activePlanRead = requestId;
    const key = rowKey(id);
    const workspace = options.workspace.value;
    planWorkspace = workspace;
    planIntent = { id, action: requestedAction };
    planVisible.value = true;
    planningAssetId.value = id;
    actionErrors.value[key] = null;
    try {
      const ready = await catalogStore.ensureReady(workspace);
      if (disposed || request !== planRequestId || workspace !== options.workspace.value) return;
      const asset = ready.inventory.assets.find((asset) => asset.stableId === id);
      const action = asset?.actions.find((action) => action.action === requestedAction);
      if (!asset) throw new Error("此资源已不在当前目录，请重新选择资源");
      if (!action?.available) throw new Error(agentAssetActionReason(action) || "当前原生操作不可用，请核对资源详情");
      const capturedGeneration = generation;
      const plan = await store.planAsset({ assetId: id, action: action.action, workspace: workspace ?? null,
        expectedRevision: asset.revision.identity, installationId: action.selectedInstallationId }, requestId, asset.agentKind);
      if (disposed || request !== planRequestId || workspace !== options.workspace.value) return;
      if (capturedGeneration !== generation) throw new Error("目录在读取期间发生变化，请重新预览当前操作");
      if (plan.assetId !== id || plan.action !== requestedAction) throw new Error("返回的计划与所选资源或操作不一致，请重新预览");
      pendingPlan.value = plan;
      now.value = Date.now();
      expirationTimer = globalThis.setInterval(() => { now.value = Date.now(); }, 1000);
    } catch (error) {
      if (!disposed && request === planRequestId && workspace === options.workspace.value) {
        cancelPlanRead();
        planError.value = agentEnvironmentErrorMessage(error);
        actionErrors.value[key] = planError.value;
      }
    } finally {
      if (request === planRequestId) { planningAssetId.value = null; activePlanRead = null; }
    }
  }
  function closePlan() {
    cancelPlanRead();
    planRequestId += 1;
    planVisible.value = false;
    pendingPlan.value = null;
    planError.value = "";
    planningAssetId.value = null;
    planIntent = null;
    if (expirationTimer !== null) globalThis.clearInterval(expirationTimer);
    expirationTimer = null;
  }
  function retryPlan() {
    const intent = planIntent;
    if (planVisible.value && intent && !planningAssetId.value) return requestPlan(intent.id, intent.action);
  }
  function confirmPlan() {
    const plan = pendingPlan.value;
    if (!plan || !canApplyPlan.value) return;
    const workspace = planWorkspace;
    closePlan();
    void store.applyAsset(plan, workspace);
  }

  async function action(kind: AgentAssetDetailTarget, id: string, requestedAction: AgentAssetActionKind) {
    const current = target(kind, id);
    const capability = current?.actions.find((item) => item.action === requestedAction && item.available);
    if (!current || !capability) return;
    switch (capability.action) {
      case "inspect":
        if (kind === "asset") openDetail(id); else openSourceDetail(id);
        return;
      case "enable":
      case "disable":
      case "remove":
        if (kind === "asset") await requestPlan(id, capability.action);
        return;
      case "preview":
        await readPreview(kind, id);
        return;
      case "open":
      case "reveal": {
        if (current.access.kind !== "ready") return;
        const confirmation: AgentAssetAccessConfirmation = {
          kind, id, action: capability.action, accessId: current.access.accessId, environmentId: current.environmentId,
          workspaceKey: workspaceKey.value, generation, label: current.label, path: current.path, risks: [...capability.risks],
        };
        if (capability.confirmationRequired || capability.risks.length) accessConfirmation.value = confirmation;
        else await executeOpen(confirmation, []);
      }
    }
  }

  async function copyPath(kind: AgentAssetDetailTarget, id: string) {
    const current = target(kind, id);
    if (!current?.path || !current.actions.some((capability) => capability.action === "inspect" && capability.available)) return;
    const capturedGeneration = generation;
    const key = rowKey(id);
    try {
      await withTimeout(clipboard.copyText(current.path), 5000, "复制路径超时");
      if (disposed || capturedGeneration !== generation) return;
      copiedPathId.value = id;
      if (copiedTimer !== null) globalThis.clearTimeout(copiedTimer);
      copiedTimer = globalThis.setTimeout(() => { copiedPathId.value = null; copiedTimer = null; }, 1500);
    } catch (error) {
      if (!disposed && capturedGeneration === generation) actionErrors.value[key] = agentEnvironmentErrorMessage(error);
    }
  }

  function invalidateTransient() {
    generation += 1;
    closePlan();
    closeAccessConfirmation();
    copiedPathId.value = null;
    actionBusy.value = {};
    actionErrors.value = {};
    actionRequests.clear();
    closeDetail();
  }
  watch(workspaceKey, invalidateTransient, { flush: "sync" });
  watch(options.inventory, () => {
    generation += 1;
    closeAccessConfirmation();
    copiedPathId.value = null;
    if (planVisible.value && !planningAssetId.value) {
      pendingPlan.value = null;
      planError.value = "目录已更新，请重新预览当前操作";
      if (expirationTimer !== null) globalThis.clearInterval(expirationTimer);
      expirationTimer = null;
    }
    if (previewState.value === "ready") {
      previewState.value = "error";
      previewError.value = "目录已更新，请重新读取当前配置";
    }
    if (selectedAssetId.value && !catalog.indexes.value.assets.has(selectedAssetId.value)) closeDetail();
    if (selectedSourceId.value && !catalog.indexes.value.sources.has(selectedSourceId.value)) closeDetail();
  }, { flush: "sync" });
  onUnmounted(() => {
    disposed = true;
    invalidateTransient();
    if (copiedTimer !== null) globalThis.clearTimeout(copiedTimer);
  });

  return {
    invalidateTransient,
    catalog, selectedAssetId, selectedSourceId, selectedDetail, selectedSource, drawerVisible,
    preview, previewState, previewError, copiedPathId, accessConfirmation, closeAccessConfirmation, confirmAccess,
    planVisible, pendingPlan, planningAssetId, planError, planExpired, canApplyPlan, closePlan, retryPlan, confirmPlan,
    operations, rowBusy, rowProgress, rowError, rowCanCancel, operationFor, cancelRow,
    operationRecoveryError: computed(() => store.operationRecoveryError),
    recoverOperations: () => store.recoverOperations(),
    cancelOperation: (id: string) => store.cancelOperation(id),
    verifyOperation: (id: string) => store.verifyOperation(id),
    operationBusy: (id: string) => {
      const operation = store.operations[id];
      return operation ? Boolean(store.operationBusy[agentAssetTargetKey(operation.assetId, store.operationTargets[id]?.workspace)]) : false;
    },
    closeDetail, openDetail, openSourceDetail, action, copyPath,
  };
}
