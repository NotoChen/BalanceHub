import { invoke } from "@tauri-apps/api/core";
export interface AgentMcpCheck {
  id: string;
  phase: "queued" | "connecting" | "initializing" | "listing" | "completed";
  outcome:
    | "passed"
    | "failed"
    | "canceled"
    | "timedOut"
    | "authenticationRequired"
    | "configurationRequired"
    | null;
  message: string;
  serverName: string | null;
  protocolVersion: string | null;
  tools: { name: string; description: string }[];
  elapsedMs: number;
}
export const startAgentMcpCheck = (connection: string) =>
  invoke<AgentMcpCheck>("start_agent_mcp_check", { connection });
export const getAgentMcpCheck = (id: string) =>
  invoke<AgentMcpCheck>("get_agent_mcp_check", { id });
export const cancelAgentMcpCheck = (id: string) =>
  invoke<AgentMcpCheck>("cancel_agent_mcp_check", { id });
