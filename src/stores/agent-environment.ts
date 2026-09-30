import { defineStore } from "pinia";
import {
  getAgentEnvironmentInventory,
  readAgentEnvironmentAsset,
  openAgentEnvironmentAsset,
  openAgentEnvironmentSource,
  readAgentEnvironmentSource,
  planAgentAsset,
  applyAgentAsset,
  getAgentAssetOperation,
  cancelAgentAssetOperation,
  listAgentAssetOperations,
  verifyAgentAssetOperation,
  type AgentEnvironmentAccessInput,
} from "../api/app";
import type {
  AgentAssetReadResult,
  AgentAssetOpenTarget,
  AgentAssetAccessRisk,
  AgentAssetPlanRequest,
  AgentAssetPlan,
  AgentAssetOperation,
  AgentAssetActionKind,
  AgentEnvironmentInventory,
  AgentInstallation,
  AgentCliKind,
} from "./provider-types";
import { withTimeout } from "../utils/promise-timeout";

type AsyncState = "idle" | "loading" | "refreshing" | "ready" | "error";
const IPC_TIMEOUT_MS = 20_000;
const OPERATION_TIMEOUT_MS = 120_000;
const OPERATION_POLL_MS = 500;

interface OperationTarget {
  assetId: string;
  action: AgentAssetActionKind;
  workspace?: string;
}

export function agentEnvironmentKey(workspace?: string) {
  return workspace?.trim() || "__native__";
}

function normalizedWorkspace(workspace?: string) {
  return workspace?.trim() || undefined;
}

export function agentAssetTargetKey(assetId: string, workspace?: string) {
  const workspaceKey = agentEnvironmentKey(workspace);
  return `${workspaceKey.length}:${workspaceKey}:${assetId}`;
}

function nextRequestId(requestIds: Record<string, number>, key: string) {
  const requestId = (requestIds[key] ?? 0) + 1;
  requestIds[key] = requestId;
  return requestId;
}

export function agentEnvironmentErrorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return error instanceof Error ? error.message : String(error);
}

function bounded<T>(promise: Promise<T>, label: string) {
  return withTimeout(promise, IPC_TIMEOUT_MS, `${label}超时，请重试`);
}

export const useAgentEnvironmentStore = defineStore("agent-environment", {
  state: () => ({
    inventories: {} as Record<string, AgentEnvironmentInventory>,
    inventoryState: {} as Record<string, AsyncState>,
    inventoryErrors: {} as Record<string, string | null>,
    previews: {} as Record<string, AgentAssetReadResult>,
    previewState: {} as Record<string, AsyncState>,
    previewErrors: {} as Record<string, string | null>,
    inventoryRequestIds: {} as Record<string, number>,
    previewRequestIds: {} as Record<string, number>,
    operations: {} as Record<string, AgentAssetOperation>,
    operationTargets: {} as Record<string, OperationTarget>,
    operationIdsByTarget: {} as Record<string, string>,
    operationBusy: {} as Record<string, boolean>,
    operationErrors: {} as Record<string, string | null>,
    operationRequestIds: {} as Record<string, number>,
    operationRecoveryState: "idle" as AsyncState,
    operationRecoveryError: null as string | null,
  }),
  getters: {
    inventory: (state) => (workspace?: string) => {
      const key = agentEnvironmentKey(workspace);
      const inventory = state.inventories[key];
      return inventory ?? null;
    },
    inventoryLoading: (state) => (workspace?: string) => {
      const status = state.inventoryState[agentEnvironmentKey(workspace)];
      return status === "loading" || status === "refreshing";
    },
    installationById: (state) => (id: string, workspace?: string) => {
      const key = agentEnvironmentKey(workspace);
      const inventory = state.inventories[key];
      return inventory
        ? inventory.installations.find((item) => item.id === id) ?? null
        : null;
    },
  },
  actions: {
    beginInventoryPublication(workspace?: string) {
      const key = agentEnvironmentKey(workspace);
      const requestId = nextRequestId(this.inventoryRequestIds, key);
      this.inventoryState[key] = this.inventories[key] ? "refreshing" : "loading";
      this.inventoryErrors[key] = null;
      return requestId;
    },
    publishInventory(inventory: AgentEnvironmentInventory, workspace: string | undefined, requestId: number) {
      const key = agentEnvironmentKey(workspace);
      if (this.inventoryRequestIds[key] !== requestId) return false;
      this.inventories[key] = inventory;
      this.inventoryState[key] = "ready";
      return true;
    },
    rejectInventoryPublication(error: unknown, workspace: string | undefined, requestId: number) {
      const key = agentEnvironmentKey(workspace);
      if (this.inventoryRequestIds[key] !== requestId) return;
      this.inventoryState[key] = "error";
      this.inventoryErrors[key] = agentEnvironmentErrorMessage(error);
    },
    async loadInventory(workspace?: string, forceRefresh = false) {
      const normalized = normalizedWorkspace(workspace);
      const key = agentEnvironmentKey(normalized);
      if (!forceRefresh && this.inventories[key] && this.inventoryState[key] === "ready") {
        return this.inventories[key];
      }
      const requestId = this.beginInventoryPublication(normalized);
      try {
        const result = await bounded(getAgentEnvironmentInventory(normalized), "Agent 盘点");
        // Rust may canonicalize a workspace alias. The request key owns this
        // cache slot; access and plan calls use the published workspace value.
        this.publishInventory(result, normalized, requestId);
        return result;
      } catch (error) {
        this.rejectInventoryPublication(error, normalized, requestId);
        throw error;
      }
    },
    async readConfigPreview(id: string, access: AgentEnvironmentAccessInput, kind: "source" | "asset" = "source") {
      const normalized = normalizedWorkspace(access.workspace);
      const key = agentAssetTargetKey(`${kind}:${id}:${access.accessId}`, normalized);
      const requestId = nextRequestId(this.previewRequestIds, key);
      this.previewState[key] = this.previews[key] ? "refreshing" : "loading";
      this.previewErrors[key] = null;
      try {
        const input = { ...access, workspace: normalized };
        const result = await bounded(kind === "source"
          ? readAgentEnvironmentSource(id, input)
          : readAgentEnvironmentAsset(id, input), "配置预览");
        if (result.stableId !== id) throw new Error("配置预览响应与请求目标不一致");
        if (result.accessId !== access.accessId) throw new Error("配置预览响应与请求访问凭据不一致");
        if (this.previewRequestIds[key] !== requestId) return result;
        this.previews[key] = result;
        const keys = Object.keys(this.previews);
        for (const staleKey of keys.slice(0, Math.max(0, keys.length - 24))) {
          delete this.previews[staleKey];
          delete this.previewState[staleKey];
          delete this.previewErrors[staleKey];
        }
        this.previewState[key] = "ready";
        return result;
      } catch (error) {
        if (this.previewRequestIds[key] !== requestId) throw error;
        this.previewState[key] = "error";
        this.previewErrors[key] = agentEnvironmentErrorMessage(error);
        throw error;
      }
    },
    openAsset(assetId: string, access: AgentEnvironmentAccessInput, target: AgentAssetOpenTarget = "asset", acceptedRisks: AgentAssetAccessRisk[] = []) {
      return bounded(openAgentEnvironmentAsset(assetId, { ...access, workspace: normalizedWorkspace(access.workspace) }, target, acceptedRisks), "打开资产");
    },
    openSource(sourceId: string, access: AgentEnvironmentAccessInput, target: AgentAssetOpenTarget = "asset", acceptedRisks: AgentAssetAccessRisk[] = []) {
      return bounded(openAgentEnvironmentSource(sourceId, { ...access, workspace: normalizedWorkspace(access.workspace) }, target, acceptedRisks), "打开配置来源");
    },
    async planAsset(request: AgentAssetPlanRequest, requestId: string, agentKind: AgentCliKind) {
      const plan = await bounded(planAgentAsset(request, requestId, agentKind), "生成变更计划");
      if (plan.assetId !== request.assetId || plan.action !== request.action) throw new Error("变更计划与请求目标不一致");
      return plan;
    },
    acceptOperation(operation: AgentAssetOperation, expected?: OperationTarget, expectedId?: string) {
      if (expectedId && operation.id !== expectedId) throw new Error("后台操作响应与请求编号不一致");
      if (expected && (operation.assetId !== expected.assetId || operation.action !== expected.action)) throw new Error("后台操作响应与请求资产不一致");
      const previous = this.operations[operation.id];
      if (previous && previous.revision >= operation.revision) return previous;
      this.operations[operation.id] = operation;
      return operation;
    },
    async applyAsset(plan: AgentAssetPlan, workspace?: string) {
      const target: OperationTarget = { assetId: plan.assetId, action: plan.action, workspace: normalizedWorkspace(workspace) };
      const key = agentAssetTargetKey(target.assetId, target.workspace);
      if (this.operationBusy[key]) return null;
      const generation = nextRequestId(this.operationRequestIds, key);
      this.operationBusy[key] = true;
      this.operationErrors[key] = null;
      try {
        const operation = await withTimeout(applyAgentAsset({ planToken: plan.token, assetId: plan.assetId, action: plan.action }), IPC_TIMEOUT_MS,
          "未能确认后台操作是否已开始，请刷新资产状态；不会自动重试");
        if (this.operationRequestIds[key] !== generation) return null;
        this.acceptOperation(operation, target);
        this.operationTargets[operation.id] = target;
        this.operationIdsByTarget[key] = operation.id;
        return await this.followOperation(operation.id, target, key, generation);
      } catch (error) {
        if (this.operationRequestIds[key] === generation) this.operationErrors[key] = agentEnvironmentErrorMessage(error);
        return null;
      } finally {
        if (this.operationRequestIds[key] === generation) this.operationBusy[key] = false;
      }
    },
    async followOperation(id: string, target: OperationTarget, key: string, generation: number) {
      const deadline = Date.now() + OPERATION_TIMEOUT_MS;
      while (this.operationRequestIds[key] === generation) {
        const current = this.operations[id];
        if (current?.phase === "completed") {
          return current;
        }
        if (Date.now() >= deadline) throw new Error("后台操作等待超时，结果尚未确认；可检查结果或刷新资产，不会自动重试修改");
        await new Promise<void>((resolve) => globalThis.setTimeout(resolve, OPERATION_POLL_MS));
        if (this.operationRequestIds[key] !== generation) return null;
        const result = await withTimeout(getAgentAssetOperation(id), Math.min(IPC_TIMEOUT_MS, Math.max(1, deadline - Date.now())),
          "后台操作状态读取超时，结果尚未确认，请检查结果");
        if (this.operationRequestIds[key] !== generation) return null;
        this.acceptOperation(result, target, id);
      }
      return null;
    },
    async cancelOperation(id: string) {
      const operation = this.operations[id];
      if (!operation?.canCancel) return null;
      const target = this.operationTargets[id];
      const key = agentAssetTargetKey(operation.assetId, target?.workspace);
      try {
        const result = await bounded(cancelAgentAssetOperation(id), "取消操作");
        return this.acceptOperation(result, target, id);
      } catch (error) {
        this.operationErrors[key] = agentEnvironmentErrorMessage(error);
        return null;
      }
    },
    async verifyOperation(id: string) {
      const operation = this.operations[id];
      if (!operation) return null;
      const target = this.operationTargets[id] ?? { assetId: operation.assetId, action: operation.action };
      const key = agentAssetTargetKey(target.assetId, target.workspace);
      if (this.operationBusy[key]) return null;
      const generation = nextRequestId(this.operationRequestIds, key);
      this.operationBusy[key] = true;
      this.operationErrors[key] = null;
      try {
        const result = await bounded(verifyAgentAssetOperation(id), "检查操作结果");
        if (this.operationRequestIds[key] !== generation) return null;
        this.acceptOperation(result, target, id);
        return await this.followOperation(id, target, key, generation);
      } catch (error) {
        if (this.operationRequestIds[key] === generation) this.operationErrors[key] = agentEnvironmentErrorMessage(error);
        return null;
      } finally {
        if (this.operationRequestIds[key] === generation) this.operationBusy[key] = false;
      }
    },
    async recoverOperations() {
      if (this.operationRecoveryState === "loading") return;
      this.operationRecoveryState = "loading";
      this.operationRecoveryError = null;
      try {
        const operations = await bounded(listAgentAssetOperations(), "读取后台操作");
        for (const operation of operations) {
          this.acceptOperation(operation);
          if (!this.operationTargets[operation.id]) {
            const inventory = Object.entries(this.inventories).find(([, item]) => item.assets.some((asset) => asset.stableId === operation.assetId));
            if (inventory) this.operationTargets[operation.id] = {
              assetId: operation.assetId,
              action: operation.action,
              workspace: inventory[0] === "__native__" ? undefined : inventory[0],
            };
          }
          const target = this.operationTargets[operation.id];
          if (!target) continue;
          const key = agentAssetTargetKey(operation.assetId, target.workspace);
          const currentId = this.operationIdsByTarget[key];
          const current = currentId ? this.operations[currentId] : null;
          if (!current || current.createdAt <= operation.createdAt) this.operationIdsByTarget[key] = operation.id;
          if (operation.phase !== "completed" && !this.operationBusy[key]) {
            const generation = nextRequestId(this.operationRequestIds, key);
            this.operationBusy[key] = true;
            void this.followOperation(operation.id, target, key, generation).catch((error: unknown) => {
              if (this.operationRequestIds[key] === generation) this.operationErrors[key] = agentEnvironmentErrorMessage(error);
            }).finally(() => {
              if (this.operationRequestIds[key] === generation) this.operationBusy[key] = false;
            });
          }
        }
        this.operationRecoveryState = "ready";
      } catch (error) {
        this.operationRecoveryState = "error";
        this.operationRecoveryError = agentEnvironmentErrorMessage(error);
      }
    },
    clearTransientState() {
      this.inventoryErrors = {};
      this.previewErrors = {};
    },
  },
});

export type AgentEnvironmentAsyncState = AsyncState;
export type { AgentInstallation };
