import type { CliSessionSummary } from "../stores/provider-types";
import type { AgentSessionResumeState, AgentSessionRole, AgentSessionRow, AgentSessionSourceStatus } from "../stores/agent-session-types";

export const agentSessionRoleLabels: Record<AgentSessionRole, string> = {
  main: "主会话", subagent: "子会话", unknown: "角色未知",
};
export const agentSessionSourceLabels: Record<AgentSessionSourceStatus, string> = {
  complete: "已读取", partial: "部分结果", unavailable: "来源不可读", unsupported: "暂不支持", cancelled: "查询已取消",
};
export const agentSessionResumeLabels: Record<AgentSessionResumeState, string> = {
  queued: "等待启动", running: "正在继续", succeeded: "已启动", failed: "继续失败", cancelled: "已取消", uncertain: "结果待确认",
};

export function sessionTime(value: string | null, timeOnly = false) {
  if (!value) return "时间未知";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return timeOnly
    ? date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : date.toLocaleString();
}

export function sessionDayLabel(value: string | null) {
  const date = value ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) return "更新时间未记录";
  return date.toLocaleDateString("zh-CN", {
    year: "numeric", month: "long", day: "numeric", weekday: "short",
  });
}

export function sessionModelLabel(session: CliSessionSummary) {
  if (session.models.length > 1) return `多模型 · 最近 ${session.model || session.models[session.models.length - 1]}`;
  return session.model || "未记录模型";
}

/** Mirrors Rust row_order for display only; backend cursors own continuation. */
export function compareSessionRows(left: AgentSessionRow, right: AgentSessionRow) {
  const leftTime = Date.parse(left.session.updatedAt || "") || 0;
  const rightTime = Date.parse(right.session.updatedAt || "") || 0;
  return rightTime - leftTime || (left.sessionRef < right.sessionRef ? -1 : left.sessionRef > right.sessionRef ? 1 : 0);
}
