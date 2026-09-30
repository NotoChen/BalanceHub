import type { AgentCatalogAction, AgentCatalogAsset, AgentCatalogBinding, AgentCatalogDrift, AgentCatalogUnresolvedTarget, AgentCatalogRelationMutation, AgentCatalogOperation } from "../stores/agent-catalog-types";
import type { AgentAssetCategory, AgentCliKind } from "../stores/provider-types";
import type { AgentWorkspacePage } from "../stores/agent-workspace";
import type { AgentLifecycleVersionState } from "../stores/agent-lifecycle-types";
import { formatAgentAssetDiagnostics } from "./agent-environment-diagnostics";
import { agentAssetProvenanceLabel, type AgentAssetProvenanceEntry } from "./agent-asset-provenance";
import { agentAssetScopeLabels } from "../composables/useAgentAssetCatalog";
import { agentAssetOperationOutcomeLabels, agentAssetOperationPhaseLabels } from "../composables/useAgentAssetConsole";

export const agentCatalogDriftLabels: Record<AgentCatalogDrift, string> = {
  observed: "原生配置", inSync: "已应用当前版本", updateAvailable: "共享定义有新版",
  modified: "原生配置已修改", missing: "原生配置缺失", unknown: "差异待确认",
};
export const agentCatalogSyncLabels = {
  current: "已同步", different: "内容不同", unknown: "内容待核对", missing: "尚未配置",
} as const;

export const agentCatalogActionLabels: Record<AgentCatalogAction, string> = {
  applyDefinition: "配置到 Agent", enable: "启用", disable: "停用", removeBinding: "移除配置",
};
export const agentCatalogRelationActionLabels: Record<AgentCatalogRelationMutation["kind"], string> = {
  merge: "合并展示", keepSeparate: "不再提醒", detach: "解除手动合并", restoreHint: "恢复提示",
};
export function agentCatalogDefinitionChangeDetail(operation: AgentCatalogOperation) {
  const change = operation.definitionChange;
  if (!change) return "";
  const version = change?.version === null || change?.version === undefined ? "" : ` v${change.version}`;
  const state = { pending: "共享版本等待保存", saved: `共享版本${version} 已保存`, unchanged: "共享版本未改变", unknown: "共享版本保存结果待核对" }[change.state];
  return [state, change.message].filter(Boolean).join(" · ");
}
export function agentCatalogOperationDetail(operation: AgentCatalogOperation) {
  const definition = agentCatalogDefinitionChangeDetail(operation);
  const verified = operation.targets.filter((target) => target.outcome === "appliedVerified").length;
  const targets = operation.targets.map((target) => `${target.label}：${target.outcome ? agentAssetOperationOutcomeLabels[target.outcome] : agentAssetOperationPhaseLabels[target.phase]}${target.message ? `（${target.message}）` : ""}`);
  return [definition, `${verified}/${operation.targets.length} 个目标已验证`, ...targets].filter(Boolean).join("；");
}
export const agentLifecycleVersionLabels: Record<AgentLifecycleVersionState, string> = {
  notChecked: "未检查", unsupported: "暂不支持检查", unknown: "版本待确认", checkFailed: "检查失败", upToDate: "已是最新",
  updateAvailable: "有更新", aheadOfLatest: "高于最新版本",
};

export function agentCatalogCategoryMatches(category: AgentAssetCategory, page: AgentWorkspacePage) {
  return page === "extension" ? category === "plugin" || category === "extension" : category === page;
}

export function agentCatalogHasAgent(asset: AgentCatalogAsset, agent: AgentCliKind) {
  return asset.bindings.some((binding) => binding.native.agentKind === agent)
    || asset.unresolvedTargets.some((target) => target.agentKind === agent);
}

export function agentCatalogUnresolvedLabel(target: AgentCatalogUnresolvedTarget) {
  const state = target.state === "suspended" ? "已停用，定义已保留" : target.state === "missing" ? "当前缺失" : "状态未知";
  return `已应用 v${target.appliedVersion} · ${state}`;
}

function matchesQuery(query: string, values: string[]) {
  const needle = query.trim().toLocaleLowerCase();
  return !needle || values.some((value) => value.toLocaleLowerCase().includes(needle));
}

export function agentCatalogBindingMatches(
  asset: AgentCatalogAsset, binding: AgentCatalogBinding, query: string,
  labels: ReadonlyMap<AgentCliKind, string>, entry: AgentAssetProvenanceEntry | null,
) {
  if (!query.trim()) return true;
  const { native } = binding;
  const variant = asset.variants.find((item) => item.id === binding.variantId);
  const scope = entry ? entry.evidence.scope : native.scope;
  return matchesQuery(query, [
    asset.name, variant?.label ?? "", ...(variant?.summary ?? []), native.label, native.nativeId,
    ...hookSearchValues(asset, binding.id),
    labels.get(native.agentKind) ?? native.agentKind, ...formatAgentAssetDiagnostics(native.diagnostics),
    scope, agentAssetScopeLabels[scope],
    ...(entry ? [agentAssetProvenanceLabel(entry.evidence), entry.source?.label ?? "", entry.source?.path ?? ""] : [native.path ?? ""]),
  ]);
}

export function agentCatalogUnresolvedMatches(
  asset: AgentCatalogAsset, target: AgentCatalogUnresolvedTarget, query: string,
  labels: ReadonlyMap<AgentCliKind, string>,
) {
  if (!query.trim()) return true;
  return matchesQuery(query, [asset.name, ...hookSearchValues(asset, target.targetId), labels.get(target.agentKind) ?? target.agentKind,
    target.scope, agentAssetScopeLabels[target.scope], agentCatalogUnresolvedLabel(target), target.message]);
}

/** Unapplied definitions have searchable content but no native provenance. */
export function agentCatalogDefinitionMatches(asset: AgentCatalogAsset, query: string) {
  if (!query.trim()) return true;
  return matchesQuery(query, [asset.name, ...hookSearchValues(asset), ...asset.variants.flatMap((variant) => [variant.label, ...variant.summary])]);
}

function hookSearchValues(asset: AgentCatalogAsset, bindingId?: string) {
  return [...(asset.hook?.rules.flatMap((rule) => [rule.name, rule.event, rule.matcher ?? "", rule.execution]) ?? []),
    ...(asset.hook?.sources.filter((source) => bindingId === undefined || source.bindingId === bindingId).map((source) => source.label) ?? [])];
}
