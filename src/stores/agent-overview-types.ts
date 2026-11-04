import type { AgentCliKind, CliToolProbeResult } from "./provider-types";
import type { AgentConfigurationSnapshot } from "./agent-configuration-types";
import type { AgentSettingValueSummary } from "./agent-settings-types";

/** Display snapshot only. Never supply its cached access/context IDs to an action. */
export interface AgentOverviewSnapshot {
  agentKind: AgentCliKind;
  probe: CliToolProbeResult;
  configuration: AgentConfigurationSnapshot | null;
  configurationError: string;
  /** Native default model, resolved the same way as the visual settings form. */
  model: AgentSettingValueSummary;
  updatedAt: string;
}
