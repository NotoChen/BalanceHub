import { invoke } from "@tauri-apps/api/core";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentOverviewSnapshot } from "../stores/agent-overview-types";

export function getCachedAgentOverview(workspace?: string) {
  return invoke<AgentOverviewSnapshot[]>("get_cached_agent_overview", { workspace: workspace || null });
}

export function refreshAgentOverview(agentKind: AgentCliKind, workspace?: string, force = false) {
  return invoke<AgentOverviewSnapshot>("refresh_agent_overview", { agentKind, workspace: workspace || null, force });
}
