import { invoke } from "@tauri-apps/api/core";
import type { AgentCliKind, AgentRuntimeScope } from "../stores/provider-types";
import type {
  AgentConfigurationEdit,
  AgentConfigurationSource,
  AgentConfigurationSourceRequest,
} from "../stores/agent-configuration-types";

export interface AgentInstructionSet {
  id: string;
  revision: string;
  name: string;
  common: string;
  additions: Partial<Record<AgentCliKind, string>>;
}
export function listAgentInstructionSets() {
  return invoke<AgentInstructionSet[]>("list_agent_instruction_sets");
}
export function saveAgentInstructionSet(
  request: Omit<AgentInstructionSet, "id" | "revision"> & {
    id: string | null;
    expectedRevision: string | null;
  },
) {
  return invoke<AgentInstructionSet>("save_agent_instruction_set", { request });
}
export function deleteAgentInstructionSet(
  id: string,
  expectedRevision: string,
) {
  return invoke<void>("delete_agent_instruction_set", { id, expectedRevision });
}
export function listAgentInstructionTargets(
  agentKind: AgentCliKind,
  workspace?: string,
  runtimeScope?: AgentRuntimeScope,
) {
  return invoke<AgentConfigurationSource[]>("list_agent_instruction_targets", {
    request: { agentKind, workspace, runtimeScope },
  });
}
export function beginAgentInstructionEdit(
  id: string,
  expectedRevision: string,
  source: AgentConfigurationSourceRequest,
  remove = false,
) {
  return invoke<AgentConfigurationEdit>("begin_agent_instruction_edit", {
    request: { id, expectedRevision, source, remove },
  });
}
