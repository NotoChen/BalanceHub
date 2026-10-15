import { invoke } from "@tauri-apps/api/core";
import type {
  AgentModelCatalog,
  AgentModelCatalogContext,
} from "../stores/agent-settings-types";

export function getAgentModelCatalog(
  context: AgentModelCatalogContext,
  refresh = false,
) {
  return invoke<AgentModelCatalog>("get_agent_model_catalog", {
    context,
    refresh,
  });
}
