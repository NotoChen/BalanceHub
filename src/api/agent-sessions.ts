import { invoke } from "@tauri-apps/api/core";
import type {
  AgentSessionCancelRequest,
  AgentSessionCountRequest,
  AgentSessionCounts,
  AgentSessionDetail,
  AgentSessionDetailRequest,
  AgentSessionPage,
  AgentSessionQuery,
  AgentSessionResumeOperation,
  AgentSessionResumeRequest,
  AgentSessionScope,
} from "../stores/agent-session-types";

export function countAgentSessions(request: AgentSessionCountRequest) {
  return invoke<AgentSessionCounts>("count_agent_sessions", { request });
}

export function getAgentSessionScope(explicitWorkdir: string | null = null) {
  return invoke<AgentSessionScope>("get_agent_session_scope", { explicitWorkdir });
}

export function queryAgentSessions(request: AgentSessionQuery) {
  return invoke<AgentSessionPage>("query_agent_sessions", { request });
}

export function getAgentSessionDetail(request: AgentSessionDetailRequest) {
  return invoke<AgentSessionDetail>("get_agent_session_detail", { request });
}

export function cancelAgentSessionQuery(request: AgentSessionCancelRequest) {
  return invoke<void>("cancel_agent_session_query", { request });
}

export const agentSessionQueryApi = {
  scope: getAgentSessionScope,
  query: queryAgentSessions,
  detail: getAgentSessionDetail,
  cancel: cancelAgentSessionQuery,
};
export type AgentSessionQueryApi = typeof agentSessionQueryApi;

export function resumeAgentSession(request: AgentSessionResumeRequest) {
  return invoke<AgentSessionResumeOperation>("resume_agent_session", { request });
}

export function getAgentSessionResumeOperation(operationId: string) {
  return invoke<AgentSessionResumeOperation>("get_agent_session_resume_operation", { operationId });
}

export function listAgentSessionResumeOperations() {
  return invoke<AgentSessionResumeOperation[]>("list_agent_session_resume_operations");
}

export function cancelAgentSessionResumeOperation(operationId: string) {
  return invoke<AgentSessionResumeOperation>("cancel_agent_session_resume_operation", { operationId });
}
