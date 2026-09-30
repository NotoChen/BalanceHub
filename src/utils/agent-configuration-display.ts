import type { AgentConfigurationActionKind, AgentConfigurationDiagnostic, AgentConfigurationErrorKind, AgentConfigurationSource } from "../stores/agent-configuration-types";

export const agentConfigurationActionLabels: Record<AgentConfigurationActionKind, string> = {
  read: "只读查看", edit: "编辑", create: "创建", open: "使用系统应用打开", reveal: "在文件管理器中显示",
};
export function agentConfigurationFileName(source: Pick<AgentConfigurationSource, "path" | "label">): string {
  return source.path.split(/[\\/]/).filter(Boolean).pop() || source.label;
}
export function agentConfigurationFileLabel(source: AgentConfigurationSource, sources: AgentConfigurationSource[]): string {
  const name = agentConfigurationFileName(source);
  const peers = sources.filter((other) => other.sourceId !== source.sourceId && agentConfigurationFileName(other) === name);
  if (!peers.length) return name;
  const parts = source.path.split(/[\\/]/).filter(Boolean);
  for (let length = 2; length <= parts.length; length += 1) {
    const suffix = parts.slice(-length).join("/");
    if (peers.every((other) => other.path.split(/[\\/]/).filter(Boolean).slice(-length).join("/") !== suffix)) return suffix;
  }
  return source.profile ? `${source.path} · ${source.profile}` : source.path;
}
export function agentConfigurationOpenAction(source: AgentConfigurationSource): AgentConfigurationActionKind | null {
  return (["edit", "create", "read"] as const).find((kind) => source.actions.some((action) => action.action === kind && action.available)) ?? null;
}
export function agentConfigurationReadOnlyReason(source: AgentConfigurationSource): string {
  return agentConfigurationOpenAction(source) === "read"
    ? source.actions.find((action) => action.action === "edit")?.reason || "文件或所在目录不可写"
    : "";
}
export function agentConfigurationSourceStatus(source: AgentConfigurationSource) {
  if (source.access.kind !== "ready") return { presence: "无法访问", access: "来源不可访问" };
  if (!source.revision.identity || !source.revision.observedAt) return { presence: "状态未确认", access: "来源状态未确认" };
  return { presence: source.revision.isMissing ? "尚未创建" : "已存在", access: "来源可访问" };
}
const errorLabels: Record<AgentConfigurationErrorKind, string> = {
  invalidRequest: "配置请求无效，请重新读取", accessExpired: "文件信息已过期，请刷新后重新打开",
  actorMismatch: "配置操作不属于当前窗口", sourceChanged: "文件已被其他操作修改，请重新读取后比较草稿",
  rootChanged: "配置目录已变化，请刷新来源", sourceUnavailable: "配置来源当前不可用", readOnly: "该来源只读",
  unsupportedFormat: "此格式暂不支持编辑", invalidSyntax: "配置语法有误，请修正后重新预览",
  unsupportedScope: "此配置范围不支持该操作",
  editExpired: "编辑会话已过期，草稿已保留，请重新读取文件",
  planExpired: "更改预览已过期，请重新预览", planConsumed: "已提交保存，请查看任务结果",
  operationNotFound: "未找到该配置任务，请刷新状态", capacityExceeded: "配置任务过多，请稍后重试",
  timeout: "配置操作超时，请刷新状态；不会自动重试写入", canceled: "操作已取消",
  writeFailed: "写入未完成，请查看各文件结果", unsupportedPlatform: "当前平台不支持此操作", internalFailure: "配置操作未能完成，请刷新状态",
};

/** Never echo arbitrary IPC/parser failures: they may include submitted credentials. */
export function agentConfigurationErrorMessage(error: unknown, fallback: string) {
  if (error && typeof error === "object" && "kind" in error && typeof error.kind === "string"
    && Object.prototype.hasOwnProperty.call(errorLabels, error.kind)) return errorLabels[error.kind as AgentConfigurationErrorKind];
  return fallback;
}

export function agentConfigurationErrorDiagnostics(error: unknown): AgentConfigurationDiagnostic[] {
  if (!error || typeof error !== "object" || !("kind" in error) || typeof error.kind !== "string" || !Object.prototype.hasOwnProperty.call(errorLabels, error.kind)
    || !("diagnostics" in error) || !Array.isArray(error.diagnostics)) return [];
  return error.diagnostics.filter((value: unknown): value is AgentConfigurationDiagnostic => {
    if (!value || typeof value !== "object") return false;
    return "code" in value && typeof value.code === "string" && "message" in value && typeof value.message === "string"
      && "severity" in value && (value.severity === "info" || value.severity === "warning" || value.severity === "error")
      && "sourceId" in value && (value.sourceId === null || typeof value.sourceId === "string")
      && "line" in value && (value.line === null || typeof value.line === "number")
      && "column" in value && (value.column === null || typeof value.column === "number");
  });
}
