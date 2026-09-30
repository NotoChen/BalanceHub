import { invoke } from "@tauri-apps/api/core";
import type {
  AgentLifecycleApplyRequest,
  AgentLifecycleCatalog,
  AgentLifecycleCatalogRequest,
  AgentLifecycleOperation,
  AgentLifecyclePlan,
  AgentLifecyclePlanRequest,
} from "../stores/agent-lifecycle-types";

export function getAgentLifecycleCatalog(request: AgentLifecycleCatalogRequest = {}) {
  return invoke<AgentLifecycleCatalog>("get_agent_lifecycle_catalog", { request });
}

export function planAgentLifecycle(request: AgentLifecyclePlanRequest) {
  return invoke<AgentLifecyclePlan>("plan_agent_lifecycle", { request });
}

// Confirmation UIs close before calling this; execution continues in the backend.
export function applyAgentLifecycle(request: AgentLifecycleApplyRequest) {
  return invoke<AgentLifecycleOperation>("apply_agent_lifecycle", { request });
}

export function getAgentLifecycleOperation(operationId: string) {
  return invoke<AgentLifecycleOperation>("get_agent_lifecycle_operation", { operationId });
}

export function listAgentLifecycleOperations() {
  return invoke<AgentLifecycleOperation[]>("list_agent_lifecycle_operations");
}

export function cancelAgentLifecycleOperation(operationId: string) {
  return invoke<AgentLifecycleOperation>("cancel_agent_lifecycle_operation", { operationId });
}
