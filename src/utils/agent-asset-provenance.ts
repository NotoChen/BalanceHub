import type {
  AgentAssetInstallationOrigin, AgentAssetProvenance, AgentAssetProviderOrigin,
  AgentAssetProvision, AgentAssetSource,
} from "../stores/provider-types";

export const agentAssetProvisionLabels: Record<AgentAssetProvision, string> = {
  agentBuiltIn: "Agent 自带", pluginProvided: "插件提供", independent: "独立配置", unknown: "提供方式待确认",
};
export const agentAssetInstallationLabels: Record<AgentAssetInstallationOrigin, string> = {
  bundled: "随 Agent 内置", nativePackage: "原生扩展包", localFiles: "本地文件",
  sharedFiles: "共享目录", linked: "链接引用", configEntry: "配置文件注册", unknown: "安装来源待确认",
};
export const agentAssetProviderLabels: Record<AgentAssetProviderOrigin, string> = {
  agentVendor: "Agent 官方", thirdParty: "第三方提供", userDeclared: "用户声明自制", unknown: "作者未确认",
};

export function agentAssetProvenanceLabel(evidence: AgentAssetProvenance): string {
  const labels = [agentAssetProvisionLabels[evidence.provision], agentAssetInstallationLabels[evidence.installation]];
  if (evidence.provider !== "unknown") labels.push(agentAssetProviderLabels[evidence.provider]);
  return labels.join(" · ");
}

/** Match one displayed fact to its own provenance, without combining Agent evidence. */
export function agentAssetFeatureMatches(feature: string | null, evidence: AgentAssetProvenance) {
  return feature === `provision-${evidence.provision}` || feature === `installation-${evidence.installation}` || feature === `provider-${evidence.provider}`;
}

export interface AgentAssetProvenanceEntry {
  evidence: AgentAssetProvenance;
  source: AgentAssetSource | null;
}

// Published inventories replace their source arrays; let old snapshot indexes be collected.
const sourceIndexes = new WeakMap<readonly AgentAssetSource[], ReadonlyMap<string, AgentAssetSource>>();

/** Resolve each supply definition's own source without borrowing control overlays. */
export function agentAssetProvenanceEntries(
  evidence: readonly AgentAssetProvenance[], sources: readonly AgentAssetSource[],
): AgentAssetProvenanceEntry[] {
  if (!evidence.length) return [];
  let byId = sourceIndexes.get(sources);
  if (!byId) {
    byId = new Map(sources.map((source) => [source.id, source]));
    sourceIndexes.set(sources, byId);
  }
  return evidence.map((item) => ({ evidence: item, source: byId.get(item.sourceId) ?? null }));
}
