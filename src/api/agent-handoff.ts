import { invoke } from "./transport.ts";
import type {
  AgentHandoff,
  AgentHandoffRequest,
} from "../stores/agent-handoff-types";

export function prepareAgentHandoff(request: AgentHandoffRequest) {
  return invoke<AgentHandoff>("prepare_agent_handoff", { request });
}
