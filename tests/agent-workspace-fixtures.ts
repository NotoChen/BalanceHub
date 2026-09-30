import type {
  AgentAssetCatalog, AgentCatalogAsset, AgentCatalogBinding, AgentCatalogDefinition,
  AgentCatalogOperation, AgentCatalogPlan, AgentCatalogTarget, AgentCatalogAgentObservation,
  AgentCatalogAgentPanel, AgentCatalogRelationPreview,
} from "../src/stores/agent-catalog-types.ts";
import type {
  AgentLifecycleCatalog, AgentLifecycleOperation, AgentLifecyclePlan, AgentLifecycleTarget,
} from "../src/stores/agent-lifecycle-types.ts";
import type { AgentCliDescriptor, AgentCliKind, AgentHookInspection } from "../src/stores/provider-types.ts";
import { assetContext, assetInstallation, assetInventory, assetRecord, assetSource } from "./agent-asset-fixtures.ts";

export function workspaceAgent(kind: AgentCliKind = "codex"): AgentCliDescriptor {
  return { kind, label: `${kind === "codex" ? "Codex" : kind === "claudeCode" ? "Claude" : kind} CLI`, executable: kind === "claudeCode" ? "claude" : kind, sessionNameHint: "fixture",
    capabilities: { temporaryLaunch: true, modelSelection: true, sessionHistory: true, sessionSearch: true,
      sessionDetail: true, sessionResume: true, sessionName: true, liveness: true, defaultConfig: true } };
}

export function catalogBinding(kind: AgentCliKind = "codex", id = `binding-${kind}`): AgentCatalogBinding {
  const native = assetRecord(`native-${id}`, { agentKind: kind, contextId: `context-${kind}`, nativeId: "shared-tool", label: "Shared tool",
    sourceIds: [`source-${kind}`], inspectionSourceId: `source-${kind}`, path: `/fixture/${kind}/config.json`,
    provenance: [{ sourceId: `source-${kind}`, declarationId: `declaration:native-${id}`, scope: "user",
      provision: "independent", installation: "configEntry", provider: "unknown" }],
    compatibleInstallationIds: [`installation-${kind}`], selectedActionInstallationId: `installation-${kind}` });
  return { id, native, actions: native.actions.map((action) => ({ ...action })),
  variantId: "variant-one", drift: "inSync", appliedVersion: 1, canAdopt: false, reason: null };
}

export function catalogAsset(id = "global-asset", fields: Partial<AgentCatalogAsset> = {}): AgentCatalogAsset {
  const bindings = fields.bindings ?? [catalogBinding("codex"), catalogBinding("claudeCode")];
  const unresolvedTargets = fields.unresolvedTargets ?? [];
  return { id, contentRevision: `content-${id}`, name: "Shared tool", category: "mcp", createdAt: null, modifiedAt: null, hook: null, ownership: "managed", version: 1,
    definitionRemoval: { available: false, reason: "仍有关联的原生来源，请先处理绑定" },
    application: { available: true, sourceBindingId: null, targets: [catalogTarget(), catalogTarget("claudeCode")],
      observations: catalogObservations([...bindings.map((binding) => binding.native.agentKind), ...unresolvedTargets.map((target) => target.agentKind)]), reason: null },
    provenance: { provisions: ["independent"], installations: ["configEntry"], providers: ["unknown"] },
    variants: [{ id: "variant-one", label: "共享定义", complete: true, summary: ["stdio · fixture-server"] }],
    bindings, unresolvedTargets, candidateIds: [], separatedAssetIds: [], manualAssociations: [], ...fields };
}

export function catalogObservations(observed: AgentCliKind[] = []): AgentCatalogAgentObservation[] {
  return (["codex", "claudeCode", "gemini", "grok"] satisfies AgentCliKind[]).map((agentKind) => ({
    agentKind, state: observed.includes(agentKind) ? "observed" : "missing", reason: null,
  }));
}

export function catalogTarget(kind: AgentCliKind = "codex", fields: Partial<AgentCatalogTarget> = {}): AgentCatalogTarget {
  return { id: `target-${kind}`, contextId: `context-${kind}`, agentKind: kind, scope: "user",
    label: `${workspaceAgent(kind).label} 用户配置`, categories: ["skill", "mcp"], available: true, reason: null, ...fields };
}

export function catalogSnapshot(fields: Partial<AgentAssetCatalog> = {}): AgentAssetCatalog {
  const assets = fields.assets ?? [catalogAsset()];
  const natives = assets.flatMap((asset) => asset.bindings.map((binding) => binding.native));
  const kinds = [...new Set(natives.map((native) => native.agentKind))];
  return {
    revision: "catalog-revision", counts: {}, creatableCategories: ["skill", "mcp", "hook"], assets,
    inventory: assetInventory({ assets: natives,
      contexts: kinds.map((kind) => assetContext({ id: `context-${kind}`, agentKind: kind, configRoot: `/fixture/${kind}`,
        compatibleInstallationIds: [`installation-${kind}`] })),
      sources: kinds.map((kind) => assetSource(`source-${kind}`, { contextId: `context-${kind}`, path: `/fixture/${kind}/config.json` })),
      installations: kinds.map((kind) => assetInstallation(`installation-${kind}`, { agentKind: kind, label: workspaceAgent(kind).label })),
    }),
    targets: [catalogTarget(), catalogTarget("claudeCode")],
    diagnostics: [], ...fields,
  };
}

export function catalogProvenanceSnapshot(): AgentAssetCatalog {
  const entries = [
    { kind: "codex", provision: "agentBuiltIn", installation: "bundled", provider: "agentVendor", scope: "system", state: "enabled", path: "/fixture/codex/skills/.system/review/SKILL.md" },
    { kind: "claudeCode", provision: "pluginProvided", installation: "nativePackage", provider: "unknown", scope: "user", state: "disabled", path: "/fixture/claude/plugins/review/SKILL.md" },
    { kind: "gemini", provision: "independent", installation: "sharedFiles", provider: "unknown", scope: "user", state: "enabled", path: "/fixture/shared/skills/review/SKILL.md" },
  ] as const;
  const bindings = entries.map((entry) => {
    const binding = catalogBinding(entry.kind);
    binding.variantId = `variant-${entry.kind}`;
    binding.native = { ...binding.native, category: "skill", nativeId: "review", label: "Review skill", scope: entry.scope, path: entry.path,
      effectiveState: entry.state, declaredState: entry.state, details: { kind: "skill", enabled: entry.state, invocationPolicy: "modelInvocable" },
      provenance: [{ sourceId: `source-${entry.kind}`, declarationId: `declaration:${binding.native.stableId}`, scope: entry.scope,
        provision: entry.provision, installation: entry.installation, provider: entry.provider }] };
    return binding;
  });
  const snapshot = catalogSnapshot({ assets: [catalogAsset("mixed-provenance", {
    name: "Review skill", category: "skill", bindings,
    provenance: { provisions: ["agentBuiltIn", "pluginProvided", "independent"], installations: ["bundled", "nativePackage", "sharedFiles"], providers: ["agentVendor", "unknown"] },
    variants: entries.map((entry) => ({ id: `variant-${entry.kind}`, label: `${entry.kind} 配置`, complete: true, summary: [`${entry.kind}-only-description`] })),
  })] });
  snapshot.inventory.sources = entries.map((entry) => assetSource(`source-${entry.kind}`, {
    contextId: `context-${entry.kind}`, scope: entry.scope, origin: entry.installation, path: entry.path, categories: ["skill"],
  }));
  return snapshot;
}

export function catalogDefinition(fields: Partial<AgentCatalogDefinition> = {}): AgentCatalogDefinition {
  return { assetId: "global-asset", name: "Shared skill", category: "skill", version: 1, mcp: null, hook: null,
    skillMarkdown: "---\nname: fixture\ndescription: fixture skill\n---\nUse local files.\n",
    files: [{ path: "SKILL.md", sizeBytes: 96 }, { path: "scripts/check.sh", sizeBytes: 42 }], notes: [], ...fields };
}

export function catalogPlan(fields: Partial<AgentCatalogPlan> = {}): AgentCatalogPlan {
  return { token: "catalog-plan", planId: "catalog-plan-id", assetId: "global-asset", action: "applyDefinition", version: 1, definitionChange: null,
    expiresAt: new Date(Date.now() + 60_000).toISOString(),
    targets: [{ targetId: "target-codex", label: "Codex CLI 用户配置", agentKind: "codex", contextId: "context-codex", scope: "user", targetKind: "destination", available: true, reason: null,
      affectedAssetIds: ["native-binding-codex"], changes: [{ label: "应用共享定义", path: "/fixture/codex/config.json", before: "old", after: "new" }] }],
    notes: [], ...fields };
}

export function catalogOperation(fields: Partial<AgentCatalogOperation> = {}): AgentCatalogOperation {
  const timestamp = new Date().toISOString();
  return { id: "catalog-operation", planId: "catalog-plan-id", assetId: "global-asset", action: "applyDefinition", phase: "applying", revision: 1, definitionChange: null,
    canCancel: true, createdAt: timestamp, updatedAt: timestamp,
    targets: [{ targetId: "target-codex", label: "Codex CLI 用户配置", phase: "applying", outcome: null, message: null, nativeOperationId: "native-child" }], ...fields };
}

export function catalogAgentPanel(kind: AgentCliKind = "codex", fields: Partial<AgentCatalogAgentPanel> = {}): AgentCatalogAgentPanel {
  return { assetId: "global-asset", agentKind: kind, revision: "catalog-revision",
    observation: { agentKind: kind, state: "observed", reason: null },
    entries: [{ targetId: `binding-${kind}`, targetKind: "binding", contextId: `context-${kind}`, scope: "user", label: "用户配置",
      path: `/fixture/${kind}/config.json`, stateLabel: "已启用", reason: null, diagnostics: [],
      actions: [{ action: "disable", targetIds: [`binding-${kind}`], available: true, reason: null, affectedAssetIds: [], parentNativeAssetId: null }] }],
    ...fields };
}

export function catalogRelationPreview(fields: Partial<AgentCatalogRelationPreview> = {}): AgentCatalogRelationPreview {
  return { token: null, relationKey: "relation-key", action: "compare", expiresAt: null, equality: "different",
    sides: ["global-asset", "other-asset"].map((assetId) => ({ assetId, name: "审阅", category: "skill", ownership: "observed", version: null,
      complete: true, reason: null, definition: [], bindings: [{ bindingId: "binding-" + assetId, agentKind: "codex", contextId: "context-codex", scope: "user",
        path: "/fixture/" + assetId + "/SKILL.md", provenance: [], complete: true, reason: null,
        documents: [{ label: "SKILL.md", path: "SKILL.md", format: "markdown", content: "# " + assetId + "\n", truncated: false, reason: null }] }] })),
    differences: [{ path: "SKILL.md", kind: "changed", leftSummary: "资源 1 内容", rightSummary: "资源 2 内容", reason: null }],
    capabilities: [
      { intent: { kind: "merge", destinationAssetId: "global-asset", sourceAssetId: "other-asset" }, available: true, reason: null },
      { intent: { kind: "merge", destinationAssetId: "other-asset", sourceAssetId: "global-asset" }, available: false, reason: "独立共享版本历史不能合并" },
      { intent: { kind: "keepSeparate", leftAssetId: "global-asset", rightAssetId: "other-asset", hideCandidate: true }, available: true, reason: null },
    ], affectedBindingIds: [], affectedReceiptTargetIds: [], available: true, reason: null, notes: [], ...fields };
}

export function lifecycleTarget(fields: Partial<AgentLifecycleTarget> = {}): AgentLifecycleTarget {
  return { id: "npm-codex", agentKind: "codex", label: "Codex CLI", installation: assetInstallation("installation-codex"), isCurrent: true, channel: "npm", channelLabel: "npm 全局安装",
    releaseTrack: "latest", directory: "/fixture/npm", evidenceRevision: "lifecycle-evidence",
    version: { state: "updateAvailable", source: "npmRegistry", latestVersion: "1.1.0", checkedAt: null, lastSuccessAt: null, nextCheckAt: null, stale: false, message: null },
    actions: [{ kind: "upgrade", available: true, reason: null, reasonMessage: null }], ...fields };
}

export function lifecycleCatalog(targets = [lifecycleTarget()]): AgentLifecycleCatalog {
  return { targets, refreshedAt: new Date().toISOString(), nextCheckAt: null };
}

export function lifecyclePlan(fields: Partial<AgentLifecyclePlan> = {}): AgentLifecyclePlan {
  return { planToken: "lifecycle-plan", agentKind: "codex", targetId: "npm-codex", installationId: "installation-codex", action: "upgrade",
    channel: "npm", channelLabel: "npm 全局安装", directory: "/fixture/npm", fromVersion: "1.0.0", toVersion: "1.1.0",
    mechanismId: "fixture-upgrade", changes: ["升级已有 npm 安装"], commandPreview: ["fixture-npm install --global fixture@1.1.0"],
    affectedInstallationIds: ["installation-codex"], confirmationMessage: "确认升级", cancellationBoundary: "写入前可取消", timeoutSeconds: 60,
    expiresAt: new Date(Date.now() + 60_000).toISOString(), ...fields };
}

export function lifecycleOperation(fields: Partial<AgentLifecycleOperation> = {}): AgentLifecycleOperation {
  const timestamp = new Date().toISOString();
  return { id: "lifecycle-operation", agentKind: "codex", targetId: "npm-codex", installationId: "installation-codex", action: "upgrade",
    channel: "npm", channelLabel: "npm 全局安装", directory: "/fixture/npm", fromVersion: "1.0.0", toVersion: "1.1.0",
    observedVersion: null, recovered: false, nextLaunch: null, verifiedExecutablePath: null, phase: "applying", canCancel: true, revision: 1,
    createdAt: timestamp, updatedAt: timestamp, outcome: null, message: null, timedOut: false, outputTruncated: false, ...fields };
}

export function workspaceHook(agentKind: AgentCliKind): AgentHookInspection {
  return { agentKind, runtimeScope: { kind: "native" }, configPath: `/fixture/${agentKind}/hooks.json`, configExists: false,
    revision: "hook-fixture", state: "not_installed", installed: false, enabled: false, trusted: "unknown",
    helperAvailable: true, spoolAvailable: true, lastEventAt: null, ownership: null, diagnostics: [], actions: [] };
}
