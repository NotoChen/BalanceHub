import { defineStore } from "pinia";
import {
  applyAgentHook,
  healthAgentHook,
  inspectAgentHook,
  planAgentHook,
  repairAgentHook,
  verifyAgentHook,
} from "../api/app";
import { agentHookTargetKey, type AgentHookTargetKey } from "../utils/agent-runtime";
import { withTimeout } from "../utils/promise-timeout";
import type {
  AgentCliKind,
  AgentHookInspection,
  AgentHookMutation,
  AgentHookPlan,
  AgentRuntimeScope,
} from "./provider-types";

type HookAsyncState = "idle" | "loading" | "ready" | "error";

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

const HOOK_OPERATION_TIMEOUT_MS = 15_000;

function ensureNativeScope(scope: AgentRuntimeScope) {
  if (scope.kind !== "native") {
    throw new Error("当前版本暂不支持管理 WSL Agent Hook");
  }
}

function ensureInspectionTarget(expected: AgentHookTargetKey, inspection: AgentHookInspection) {
  if (agentHookTargetKey(inspection.agentKind, inspection.runtimeScope) !== expected) {
    throw new Error("Agent Hook 检查结果与请求目标不一致");
  }
}

function ensurePlanTarget(expected: AgentHookTargetKey, plan: AgentHookPlan) {
  if (agentHookTargetKey(plan.agentKind, plan.runtimeScope) !== expected) {
    throw new Error("Agent Hook 计划与请求目标不一致");
  }
}

export const useAgentHookStore = defineStore("agent-hooks", {
  state: () => ({
    inspections: {} as Partial<Record<AgentHookTargetKey, AgentHookInspection>>,
    states: {} as Partial<Record<AgentHookTargetKey, HookAsyncState>>,
    errors: {} as Partial<Record<AgentHookTargetKey, string | null>>,
    requestIds: {} as Partial<Record<AgentHookTargetKey, number>>,
  }),
  actions: {
    nextRequestId(targetKey: AgentHookTargetKey) {
      const requestId = (this.requestIds[targetKey] ?? 0) + 1;
      this.requestIds[targetKey] = requestId;
      return requestId;
    },
    async inspect(
      agentKind: AgentCliKind,
      mode: "inspect" | "health" | "verify" = "inspect",
      scope: AgentRuntimeScope = { kind: "native" },
    ) {
      ensureNativeScope(scope);
      const targetKey = agentHookTargetKey(agentKind, scope);
      const requestId = this.nextRequestId(targetKey);
      this.states[targetKey] = "loading";
      this.errors[targetKey] = null;
      try {
        const command = mode === "health"
          ? healthAgentHook
          : mode === "verify"
            ? verifyAgentHook
            : inspectAgentHook;
        const inspection = await withTimeout(
          command(agentKind),
          HOOK_OPERATION_TIMEOUT_MS,
          "Agent Hook 检查超时",
        );
        ensureInspectionTarget(targetKey, inspection);
        if (this.requestIds[targetKey] !== requestId) return inspection;
        this.inspections[targetKey] = inspection;
        this.states[targetKey] = "ready";
        return inspection;
      } catch (error) {
        if (this.requestIds[targetKey] === requestId) {
          this.states[targetKey] = "error";
          this.errors[targetKey] = errorMessage(error);
        }
        throw error;
      }
    },
    async plan(
      agentKind: AgentCliKind,
      mutation: AgentHookMutation,
      scope: AgentRuntimeScope = { kind: "native" },
    ) {
      ensureNativeScope(scope);
      const targetKey = agentHookTargetKey(agentKind, scope);
      const plan = await withTimeout(
        planAgentHook(agentKind, mutation),
        HOOK_OPERATION_TIMEOUT_MS,
        "生成 Agent Hook 计划超时",
      );
      ensurePlanTarget(targetKey, plan);
      return plan;
    },
    async repairPlan(agentKind: AgentCliKind, scope: AgentRuntimeScope = { kind: "native" }) {
      ensureNativeScope(scope);
      const targetKey = agentHookTargetKey(agentKind, scope);
      const plan = await withTimeout(
        repairAgentHook(agentKind),
        HOOK_OPERATION_TIMEOUT_MS,
        "生成 Agent Hook 修复计划超时",
      );
      ensurePlanTarget(targetKey, plan);
      return plan;
    },
    async apply(agentKind: AgentCliKind, plan: AgentHookPlan) {
      ensureNativeScope(plan.runtimeScope);
      const targetKey = agentHookTargetKey(agentKind, plan.runtimeScope);
      ensurePlanTarget(targetKey, plan);
      const requestId = this.nextRequestId(targetKey);
      this.states[targetKey] = "loading";
      this.errors[targetKey] = null;
      try {
        const inspection = await withTimeout(
          applyAgentHook(agentKind, plan),
          HOOK_OPERATION_TIMEOUT_MS,
          "应用 Agent Hook 计划超时",
        );
        ensureInspectionTarget(targetKey, inspection);
        if (this.requestIds[targetKey] !== requestId) return inspection;
        this.inspections[targetKey] = inspection;
        this.states[targetKey] = "ready";
        return inspection;
      } catch (error) {
        if (this.requestIds[targetKey] === requestId) {
          this.states[targetKey] = "error";
          this.errors[targetKey] = errorMessage(error);
        }
        throw error;
      }
    },
  },
});
