import { invoke } from "./transport.ts";
import type {
  AgentUsageReport,
  AgentUsageReportRequest,
} from "../stores/agent-usage-types";

export function getAgentUsageReport(request: AgentUsageReportRequest) {
  return invoke<AgentUsageReport>("get_agent_usage_report", { request });
}
