import { invoke } from "@tauri-apps/api/core";
import { cancelAgentCatalogRead } from "./agent-catalog";
import type {
  AgentConfigurationApplyRequest, AgentConfigurationEdit, AgentConfigurationListRequest, AgentConfigurationOpenRequest,
  AgentConfigurationOperation, AgentConfigurationPlan, AgentConfigurationReadResult, AgentConfigurationSaveRequest,
  AgentConfigurationSnapshot, AgentConfigurationSourceRequest, AgentResourceEditRequest,
} from "../stores/agent-configuration-types";

export function listAgentConfigurationSources(request: AgentConfigurationListRequest) {
  return invoke<AgentConfigurationSnapshot>("list_agent_configuration_sources", { request });
}
export function readAgentConfigurationSource(request: AgentConfigurationSourceRequest) {
  return invoke<AgentConfigurationReadResult>("read_agent_configuration_source", { request });
}
export function beginAgentConfigurationEdit(request: AgentConfigurationSourceRequest) {
  return invoke<AgentConfigurationEdit>("begin_agent_configuration_edit", { request });
}
export function beginAgentResourceEdit(request: AgentResourceEditRequest, signal: AbortSignal) {
  if (signal.aborted) return Promise.reject(new Error("资源读取已取消"));
  const requestId = crypto.randomUUID();
  const cancel = () => { void cancelAgentCatalogRead(requestId).catch(() => {}); };
  signal.addEventListener("abort", cancel, { once: true });
  return invoke<AgentConfigurationEdit>("begin_agent_resource_edit", { request, requestId })
    .finally(() => signal.removeEventListener("abort", cancel));
}
export function openAgentResourceLink(url: string) {
  return invoke<void>("open_agent_resource_link", { url });
}
export function planAgentConfigurationSave(request: AgentConfigurationSaveRequest) {
  return invoke<AgentConfigurationPlan>("plan_agent_configuration_save", { request });
}
export function applyAgentConfigurationPlan(request: AgentConfigurationApplyRequest) {
  return invoke<AgentConfigurationOperation>("apply_agent_configuration_plan", { request });
}
export function discardAgentConfigurationEdit(editId: string) {
  return invoke<void>("discard_agent_configuration_edit", { editId });
}
export function getAgentConfigurationOperation(operationId: string) {
  return invoke<AgentConfigurationOperation>("get_agent_configuration_operation", { operationId });
}
export function listAgentConfigurationOperations() {
  return invoke<AgentConfigurationOperation[]>("list_agent_configuration_operations");
}
export function cancelAgentConfigurationOperation(operationId: string) {
  return invoke<AgentConfigurationOperation>("cancel_agent_configuration_operation", { operationId });
}
export function openAgentConfigurationSource(request: AgentConfigurationOpenRequest) {
  return invoke<void>("open_agent_configuration_source", { request });
}
