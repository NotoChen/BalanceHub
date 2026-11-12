import { invoke } from "./transport.ts";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentOverviewSnapshot } from "../stores/agent-overview-types";
import type { AgentSelfCheckReport } from "../stores/agent-self-check-types";

export function getCachedAgentOverview(workspace?: string) {
  return invoke<AgentOverviewSnapshot[]>("get_cached_agent_overview", { workspace: workspace || null });
}

export function refreshAgentOverview(agentKind: AgentCliKind, workspace?: string, force = false) {
  return invoke<AgentOverviewSnapshot>("refresh_agent_overview", { agentKind, workspace: workspace || null, force });
}

export function getAgentEnvironmentReport(workspace?: string) {
  return invoke<AgentSelfCheckReport>("get_agent_environment_report", { workspace: workspace || null });
}
