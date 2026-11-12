import { Channel, invoke } from "./transport.ts";
import type {
  AgentProjectProgress,
  AgentProjectReport,
  AgentProjectReportRequest,
} from "../stores/agent-project-types";

export function getAgentProjects(
  request: AgentProjectReportRequest,
  onProgress?: (progress: AgentProjectProgress) => void,
) {
  const progress = new Channel<AgentProjectProgress>();
  if (onProgress) progress.onmessage = onProgress;
  return invoke<AgentProjectReport>("get_agent_projects", {
    request,
    progress,
  });
}
