import { invoke } from "@tauri-apps/api/core";
import type {
  AgentHistoryEntry,
  AgentMaintenancePlan,
  AgentMaintenanceOperation,
  AgentSharedVersion,
  AgentSkillOrigin,
  AgentSkillUpdateCheck,
  AgentBatchOption,
} from "../stores/agent-maintenance-types";
import type {
  AgentCatalogAction,
  AgentCatalogPlanRequest,
} from "../stores/agent-catalog-types";
import type { AgentAssetPlanChange } from "../stores/provider-types";
import type {
  AgentMcpToolsRequest,
  AgentMcpToolRules,
} from "../stores/agent-maintenance-types";

export const listAgentHistory = (offset = 0, limit = 30) =>
  invoke<AgentHistoryEntry[]>("list_agent_history", { offset, limit });
export const getAgentMaintenanceChange = (planToken: string, index: number) =>
  invoke<AgentAssetPlanChange>("get_agent_maintenance_change", {
    planToken,
    index,
  });
export const discardAgentMaintenancePlan = (planToken: string) =>
  invoke<void>("discard_agent_maintenance_plan", { planToken });
export const getAgentMcpToolRules = (
  request: AgentMcpToolsRequest,
  requestId: string,
) =>
  invoke<AgentMcpToolRules>("get_agent_mcp_tool_rules", { request, requestId });
export const planAgentMcpTools = (
  request: AgentMcpToolsRequest,
  disabled: string[],
  requestId: string,
) =>
  invoke<AgentMaintenancePlan>("plan_agent_mcp_tools", {
    request,
    disabled,
    requestId,
  });
export const planAgentSessionRelocation = (request: {
  scopeRevision: string;
  sessionRef: string;
  destination: string;
  consumerId: string;
  requestId: number;
}) =>
  invoke<AgentMaintenancePlan>("plan_agent_session_relocation", { request });
export const planAgentHistoryRestore = (
  historyId: string,
  selectedPaths?: string[],
) =>
  invoke<AgentMaintenancePlan>("plan_agent_history_restore", {
    historyId,
    selectedPaths: selectedPaths ?? null,
  });
export const applyAgentMaintenance = (plan: AgentMaintenancePlan) =>
  invoke<AgentMaintenanceOperation>("apply_agent_maintenance", {
    request: { planToken: plan.token, kind: plan.kind },
  });
export const getAgentMaintenanceOperation = (operationId: string) =>
  invoke<AgentMaintenanceOperation>("get_agent_maintenance_operation", {
    operationId,
  });
export const listAgentMaintenanceOperations = () =>
  invoke<AgentMaintenanceOperation[]>("list_agent_maintenance_operations");
export const cancelAgentMaintenanceOperation = (operationId: string) =>
  invoke<AgentMaintenanceOperation>("cancel_agent_maintenance_operation", {
    operationId,
  });
export const listAgentSharedVersions = (assetId: string) =>
  invoke<AgentSharedVersion[]>("list_agent_shared_versions", { assetId });
export const planAgentSharedVersion = (assetId: string, version: number) =>
  invoke<AgentMaintenancePlan>("plan_agent_shared_version", {
    assetId,
    version,
  });
export const getAgentSkillOrigin = (assetId: string, workspace?: string) =>
  invoke<AgentSkillOrigin | null>("get_agent_skill_origin", {
    assetId,
    workspace: workspace ?? null,
  });
export const saveAgentSkillOrigin = (
  assetId: string,
  version: number,
  origin: AgentSkillOrigin | null,
) =>
  invoke<AgentSkillOrigin | null>("save_agent_skill_origin", {
    assetId,
    version,
    origin,
  });
export const checkAgentSkillUpdate = (assetId: string, requestId: string) =>
  invoke<AgentSkillUpdateCheck>("check_agent_skill_update", {
    assetId,
    requestId,
  });
export const planAgentSkillUpdate = (assetId: string, requestId: string) =>
  invoke<AgentMaintenancePlan>("plan_agent_skill_update", {
    assetId,
    requestId,
  });
export const getAgentBatchOptions = (
  assetIds: string[],
  action: AgentCatalogAction,
  workspace: string | undefined,
  requestId: string,
) =>
  invoke<AgentBatchOption[]>("get_agent_batch_options", {
    assetIds,
    action,
    workspace: workspace ?? null,
    requestId,
  });
export const planAgentBatch = (
  requests: AgentCatalogPlanRequest[],
  workspace: string | undefined,
  requestId: string,
) =>
  invoke<AgentMaintenancePlan>("plan_agent_batch", {
    requests,
    workspace: workspace ?? null,
    requestId,
  });
