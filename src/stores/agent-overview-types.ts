import type { AgentCliKind, CliToolProbeResult } from "./provider-types";
import type { AgentConfigurationSnapshot } from "./agent-configuration-types";

/** Display snapshot only. Never supply its cached access/context IDs to an action. */
export interface AgentOverviewSnapshot {
  agentKind: AgentCliKind;
  probe: CliToolProbeResult;
  configuration: AgentConfigurationSnapshot | null;
  configurationError: string;
  updatedAt: string;
}
