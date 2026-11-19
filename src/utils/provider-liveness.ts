import type { AgentCliKind, AppSettings, LivenessRecord, Provider } from "../stores/providers";
import { providerModelWireName } from "./model-comparison.ts";

export const LIVENESS_TIMELINE_SLOTS = 24;

/// 解析测活目标只需要这两项全局设置；卡片与详情共用，避免各自挑字段。
export type LivenessSettings = Pick<
  AppSettings,
  "livenessCliKind" | "livenessModel"
>;

export interface ProviderLivenessScope {
  /// 卡片自己声明的 Agent；跟随全局时为空，此时不按 Agent 过滤记录。
  cliKind: AgentCliKind | null;
  model: string;
  /// 该 Agent 实际请求的地址；只有卡片显式配置了 agentBaseUrls 时才不同。
  baseUrl: string;
  explicitBaseUrl: boolean;
}

/// 测活记录按 (Agent, 模型, baseUrl) 聚合：换 Agent 或换模型之后，旧记录不能
/// 继续算进当前测活线。
///
/// 目标解析必须和后端 `effective_cli_kind` / `effective_model` 一致：卡片声明
/// 为空时回落到全局设置，而不是当成「没有目标」。拿不到设置的卡片（列表里
/// 的单卡）仍只按自己的声明过滤，由明细里的 Agent/模型标签区分其他记录。
export function providerLivenessScope(provider: Provider): ProviderLivenessScope;
export function providerLivenessScope(
  provider: Provider,
  settings: LivenessSettings,
): ProviderLivenessScope & { cliKind: AgentCliKind };
export function providerLivenessScope(
  provider: Provider,
  settings?: LivenessSettings | null,
): ProviderLivenessScope;
export function providerLivenessScope(
  provider: Provider,
  settings?: LivenessSettings | null,
): ProviderLivenessScope {
  const cliKind = provider.liveness.cliKind ?? settings?.livenessCliKind ?? null;
  const model =
    provider.liveness.model.trim() || settings?.livenessModel?.trim() || "";
  const explicitBaseUrl = Boolean(
    cliKind && provider.liveness.agentBaseUrls?.[cliKind]?.trim(),
  );
  return {
    cliKind,
    model,
    baseUrl: explicitBaseUrl && cliKind
      ? provider.liveness.agentBaseUrls[cliKind]?.trim() ?? ""
      : provider.identity.baseUrl,
    explicitBaseUrl,
  };
}

function normalizedBaseUrl(value: string) {
  return value.trim().replace(/\/+$/, "").toLowerCase();
}

/// 记录里存的是实际请求地址；Codex 会补 `/v1`、Gemini 会去掉 `/v1beta`，
/// 不能直接和卡片上填写的地址做字符串相等。新旧记录都先取归一化前的地址，
/// 再用路径前缀兜底，避免一次 Agent 切换就把历史记录全部判成别的目标。
function recordBaseUrlMatches(record: LivenessRecord, scopeUrl: string): boolean {
  const recorded = record.providerBaseUrl?.trim() || record.baseUrl.trim();
  if (!recorded || !scopeUrl.trim()) return true;
  const left = normalizedBaseUrl(recorded);
  const right = normalizedBaseUrl(scopeUrl);
  return (
    left === right ||
    left.startsWith(`${right}/`) ||
    right.startsWith(`${left}/`)
  );
}

/// 记录里既有模型 ID 也有实际 wire name；配置了映射时两边都要能对上。
function recordModelCandidates(provider: Provider, model: string): string[] {
  const name = model.trim();
  if (!name) return [];
  const candidates = new Set([name.toLowerCase()]);
  candidates.add(providerModelWireName(provider, name).toLowerCase());
  return [...candidates];
}

function recordedModel(record: LivenessRecord): string {
  return (record.modelId?.trim() || record.model.trim()).toLowerCase();
}

function recordMatchesModel(
  provider: Provider,
  record: LivenessRecord,
  model: string,
): boolean {
  const candidates = recordModelCandidates(provider, model);
  return candidates.length === 0 || candidates.includes(recordedModel(record));
}

/// 只返回完全匹配当前测活目标的记录。其他 Agent 或模型的记录仍保存在
/// `liveness.records` 里，由「测活明细」统一展示；卡片和详情摘要不能把它们
/// 算进当前目标，否则换模型后旧成功率会冒充新目标的测活结果。
export function providerLivenessRecords(
  provider: Provider,
  settings?: LivenessSettings | null,
): LivenessRecord[] {
  const records = provider.liveness.records ?? [];
  const scope = providerLivenessScope(provider, settings);
  if (!scope.cliKind && !scope.model) return records;
  return records.filter((record) => {
    // 旧记录可能没有 Agent 归属；既然当前目标已经指定 Agent，就不能把
    // 无法归属的记录算进它的成功率，明细里仍会保留这些记录。
    if (scope.cliKind && record.cliKind !== scope.cliKind) return false;
    if (!recordMatchesModel(provider, record, scope.model)) return false;
    if (scope.explicitBaseUrl && !recordBaseUrlMatches(record, scope.baseUrl)) return false;
    return true;
  });
}

export function providerLivenessEnabled(provider: Provider, settings: AppSettings) {
  if (!provider.runtime.enabled || !provider.auth.apiKey.trim()) {
    return false;
  }
  return provider.liveness.useGlobal
    ? settings.livenessEnabled
    : provider.liveness.enabled;
}

/// 测活时间在存储里有毫秒数字串和 RFC3339 两种写法；排序、过滤和展示统一走
/// 这里，避免某个界面把 ISO 时间当成 NaN 后静默丢记录。
export function parseLivenessTimestamp(value: string): number {
  const numeric = Number(value);
  if (Number.isFinite(numeric)) return numeric;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function livenessTimestamp(record: LivenessRecord): number {
  return parseLivenessTimestamp(record.checkedAt);
}

/// 取某个 Agent 最近一次测活：优先匹配模型和地址，其次匹配模型，最后退回
/// 该 Agent 的其他记录。不会为了凑一个读数去拿别的 Agent 的成败。
export function latestAgentLivenessRecord(
  provider: Provider,
  kind: AgentCliKind,
  model = "",
  baseUrl = "",
): LivenessRecord | null {
  const records = (provider.liveness?.records ?? [])
    .filter((record) => record.cliKind === kind || !record.cliKind)
    .sort((left, right) => livenessTimestamp(right) - livenessTimestamp(left));
  const sameModel = (record: LivenessRecord) =>
    recordMatchesModel(provider, record, model);
  const sameUrl = (record: LivenessRecord) =>
    !baseUrl.trim() || !record.baseUrl.trim() || recordBaseUrlMatches(record, baseUrl);
  const exact =
    records.find((record) => sameModel(record) && sameUrl(record)) ?? null;
  // 预检失败（缺少 Key、CLI 找不到）发生在拿到具体模型之前，记录里没有模型；
  // 只要它比最近一次匹配记录更新，就应该显示失败，而不是继续报上次成功。
  const newestWithoutKind = records.find((record) => !record.cliKind);
  if (
    newestWithoutKind &&
    (!exact || livenessTimestamp(newestWithoutKind) > livenessTimestamp(exact))
  ) {
    return newestWithoutKind;
  }
  return (
    exact ??
    records.find(sameModel) ??
    records.find(sameUrl) ??
    records[0] ??
    null
  );
}

/// 取最近 24 次自动测活，按时间从旧到新排列。卡片上每一格就是一次测活，
/// 不按小时合并，所以格子数、成功数和明细里的次数始终对得上。
export function livenessTimelineSlots(
  records: LivenessRecord[],
  limit = LIVENESS_TIMELINE_SLOTS,
): Array<LivenessRecord | null> {
  const recent = records
    .filter((record) => record.source === "automatic")
    .filter((record) => livenessTimestamp(record) > 0)
    .sort((left, right) => livenessTimestamp(left) - livenessTimestamp(right))
    .slice(-limit);

  // 最右一格是最新一次；不足 24 次时左侧留空，而不是把记录挤到左边。
  const slots: Array<LivenessRecord | null> = Array.from({ length: limit }, () => null);
  const offset = limit - recent.length;
  recent.forEach((record, index) => {
    slots[offset + index] = record;
  });
  return slots;
}
