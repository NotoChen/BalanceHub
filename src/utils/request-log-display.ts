import type { ProviderRequestLog, ProviderRequestLogsResult } from "../stores/provider-types";
import { formatQuotaValue } from "./provider-display";

export function logIdentity(log: ProviderRequestLog, index: number) {
  return log.id || log.requestId || `${log.createdAt}-${index}`;
}

export function logTokenTotal(log: ProviderRequestLog) {
  return log.tokenUsed || log.promptTokens + log.completionTokens;
}

function isMeaningfulText(value: unknown) {
  const text = String(value ?? "").trim();
  return text !== "" && text !== "-" && text !== "—" && text.toLowerCase() !== "null";
}

export function rawValue(log: ProviderRequestLog, key: string) {
  return log.raw?.[key];
}

export function logHasUsageData(log: ProviderRequestLog) {
  return (
    isMeaningfulText(log.modelName) ||
    isMeaningfulText(log.tokenName) ||
    logTokenTotal(log) > 0 ||
    log.quota !== 0 ||
    (typeof log.durationMs === "number" && Number.isFinite(log.durationMs) && log.durationMs > 0)
  );
}

export function logDuration(log: ProviderRequestLog) {
  if (typeof log.durationMs !== "number" || !Number.isFinite(log.durationMs) || log.durationMs <= 0) {
    return "-";
  }
  if (log.durationMs >= 1000) {
    return `${(log.durationMs / 1000).toFixed(2)}s`;
  }
  return `${log.durationMs}ms`;
}

export function logDetails(log: ProviderRequestLog) {
  if (isMeaningfulText(log.content)) {
    return log.content.trim();
  }
  return "";
}

export function logRequestId(log: ProviderRequestLog) {
  const upstreamRequestId = rawValue(log, "upstream_request_id");
  if (isMeaningfulText(log.requestId)) {
    return log.requestId;
  }
  if (isMeaningfulText(upstreamRequestId)) {
    return String(upstreamRequestId);
  }
  return "";
}

export function logChannel(log: ProviderRequestLog) {
  if (isMeaningfulText(log.channel) && log.channel !== "0") {
    return log.channel;
  }
  const channelId = rawValue(log, "channel");
  if (isMeaningfulText(channelId) && String(channelId) !== "0") {
    return `#${channelId}`;
  }
  return "";
}

function logTypeCode(log: ProviderRequestLog) {
  const rawType = rawValue(log, "type");
  const code = Number(rawType ?? log.status);
  return Number.isFinite(code) ? code : null;
}

export function logTypeLabel(log: ProviderRequestLog) {
  const code = logTypeCode(log);
  const labels: Record<number, string> = {
    0: "未知",
    1: "充值",
    2: "消耗",
    3: "管理",
    4: "系统",
    5: "错误",
    6: "退款",
    7: "登录",
  };
  if (code !== null && labels[code]) {
    return labels[code];
  }
  return isMeaningfulText(log.status) ? log.status : "未知";
}

export function logStatusTone(log: ProviderRequestLog) {
  const typeCode = logTypeCode(log);
  if (typeCode === 2) return "ok";
  if (typeCode === 5) return "error";
  if (typeCode === 6) return "refund";
  if (typeCode === 1 || typeCode === 4 || typeCode === 7) return "info";

  const status = String(log.status || "").toLowerCase();
  const statusCode = Number(status);
  if (Number.isFinite(statusCode)) {
    if (statusCode >= 200 && statusCode < 400) return "ok";
    if (statusCode >= 400) return "error";
  }
  if (status.includes("success") || status === "ok" || status === "200") {
    return "ok";
  }
  if (status.includes("fail") || status.includes("error")) {
    return "error";
  }
  return "neutral";
}

export function formatLogQuotaValue(value: number, quotaDisplay?: ProviderRequestLogsResult["quotaDisplay"]) {
  return quotaDisplay ? formatQuotaValue(value, quotaDisplay, Math.abs(value) >= 1 ? 4 : 6) : "—";
}

export function logPreview(log: ProviderRequestLog) {
  return logDetails(log) || logRequestId(log) || "查看详情";
}

export function rawJson(log: ProviderRequestLog) {
  return JSON.stringify(log.raw, null, 2);
}
