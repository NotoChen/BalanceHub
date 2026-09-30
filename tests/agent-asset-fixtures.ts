import type {
  AgentAssetAction,
  AgentAssetActionKind,
  AgentAssetDeclaration,
  AgentAssetOperation,
  AgentAssetPlan,
  AgentAssetReadResult,
  AgentAssetRecord,
  AgentAssetRevision,
  AgentAssetSource,
  AgentConfigurationContext,
  AgentEnvironmentInventory,
  AgentInstallation,
} from "../src/stores/provider-types.ts";

export const assetRevision: AgentAssetRevision = {
  identity: "fixture-revision", observedAt: "2026-09-13T00:00:00Z", sizeBytes: 42, isMissing: false, isDirectory: false, isSymlink: false,
};
export function assetAction(action: AgentAssetActionKind, fields: Partial<AgentAssetAction> = {}): AgentAssetAction {
  return { action, available: true, reason: null, mechanismId: null, confirmationRequired: false, reloadEffect: null, trustEffect: null, selectedInstallationId: null, risks: [], ...fields };
}
export function assetSource(id = "source-a", fields: Partial<AgentAssetSource> = {}): AgentAssetSource {
  return {
    id, contextId: "context-a", label: `${id}.toml`, scope: "user", origin: "configEntry", environmentId: "native:fixture", workspaceId: null,
    path: `/fixture/config/${id}.toml`, precedence: 1, writable: true, sensitive: false, sourceKind: "file", categories: ["mcp"],
    revision: assetRevision, diagnostics: [], access: { kind: "ready", accessId: `access:${id}` },
    actions: [assetAction("inspect"), assetAction("preview"), assetAction("open", { confirmationRequired: true, risks: ["externalPathnameRace"] }), assetAction("reveal", { confirmationRequired: true, risks: ["externalPathnameRace"] })],
    ...fields,
  };
}
export function assetRecord(id = "asset-a", fields: Partial<AgentAssetRecord> = {}): AgentAssetRecord {
  return {
    stableId: id, agentKind: "codex", category: "mcp", nativeId: id, label: `${id} MCP`, sourceIds: ["source-a"], inspectionSourceId: "source-a",
    scope: "user", environmentId: "native:fixture", workspaceId: null, path: "/fixture/config/source-a.toml", precedence: 1, writable: true,
    provenance: [{ sourceId: "source-a", declarationId: `declaration:${id}`, provision: "independent", installation: "configEntry", provider: "unknown" }],
    declaredState: "disabled", effectiveState: "disabled", trustState: "trusted", diagnostics: [], revision: assetRevision, sensitive: false, isDirectory: false,
    contextId: "context-a", representedDeclarationIds: [`declaration:${id}`],
    resolution: { relation: "independent", qualifiedCollision: false, terminal: null, contributorIds: [`declaration:${id}`], winnerId: null, controlSource: null, diagnostics: [] },
    relationships: { providedBy: null, actionOwner: null, affectedAssetIds: [] },
    actions: [assetAction("inspect"), assetAction("preview"), assetAction("open", { confirmationRequired: true, risks: ["externalPathnameRace", "rawSensitiveContent"] }), assetAction("enable", { mechanismId: "fixture-mechanism", confirmationRequired: true, selectedInstallationId: "installation-a" })],
    access: { kind: "ready", accessId: `access:${id}` }, compatibleInstallationIds: ["installation-a", "installation-b"], selectedActionInstallationId: "installation-a",
    details: { kind: "mcp", transport: "stdio", declaredState: "disabled", approvalState: "notRequired", effectiveAvailability: "disabled" },
    ...fields,
  };
}
export function assetInstallation(id = "installation-a", fields: Partial<AgentInstallation> = {}): AgentInstallation {
  return {
    id, environmentId: "native:fixture", agentKind: "codex", label: "Codex CLI", availability: "available", executablePath: `/fixture/bin/${id}`,
    executableIdentity: { owner: "fixture", canonicalPath: `/fixture/bin/${id}`, installationSource: "configured" }, executableRevision: "fixture-executable",
    installedVersion: "1.0.0", discoverySource: "configured", distribution: "npm", channel: "stable", installedVersionSource: "localExecutable",
    diagnostics: [], ...fields,
  };
}
export function assetContext(fields: Partial<AgentConfigurationContext> = {}): AgentConfigurationContext {
  return { id: "context-a", environmentId: "native:fixture", agentKind: "codex", configRoot: "/fixture/config", profile: "default", workspaceId: null, trustContext: "trusted", parserVersion: 1, schemaFacts: {}, compatibleInstallationIds: ["installation-a", "installation-b"], ...fields };
}
export function assetDeclaration(asset: AgentAssetRecord): AgentAssetDeclaration {
  return { id: `declaration:${asset.stableId}`, contextId: asset.contextId, sourceId: asset.sourceIds[0], scope: asset.scope, nativeKind: asset.category, nativeId: asset.nativeId,
    declarationKey: asset.nativeId, label: asset.label, precedence: 1, presence: "present", declaredState: "disabled", trustState: "trusted", role: "definition", participation: { kind: "participates" },
    evidence: { revision: assetRevision, parserVersion: 1, observedAt: "2026-09-13T00:00:00Z", facts: {} }, diagnostics: [], providedBy: null, actionOwner: null, explicitlyAffected: [],
  };
}
export function assetInventory(fields: Partial<AgentEnvironmentInventory> = {}): AgentEnvironmentInventory {
  const assets = fields.assets ?? [assetRecord(), assetRecord("asset-b")];
  return {
    environment: { id: "native:fixture", kind: "native", hostPlatform: "macos", hostArchitecture: "aarch64", guestPlatform: null, displayName: "本机测试环境", capabilities: ["readOnlyInventory", "boundedPreview"] },
    installations: [assetInstallation(), assetInstallation("installation-b")], sources: [assetSource(), assetSource("source-only")], capabilities: [], assets, hookRuleCounts: [{ agentKind: "codex", ruleCount: 0 }],
    scannedAt: "2026-09-13T00:00:00Z", workspace: null, contexts: [assetContext()], declarations: assets.map(assetDeclaration), diagnostics: [],
    limits: { candidatePathsPerAgent: 32, installationsPerAgent: 8, sourcesPerContext: 128, firstLevelEntries: 512, bytesPerSource: 524288, bytesPerRefresh: 8388608, refreshBudgetMs: 5000, cliOutputBytes: 262144, cliConcurrency: 2, watchers: 64, diagnostics: 100 },
    mechanisms: [{ id: "fixture-mechanism", agentKind: "codex", category: "mcp", action: "enable", platforms: ["macos"], adapterSchemaVersion: 1, sourceSchema: "fixture-only", executableArgv: [], scopes: ["user"], inspection: "重新读取目标节点", idempotent: true, commitPoint: "原子提交", reloadEffect: "重启 Agent 后生效", redactionRules: [] }],
    ...fields,
  };
}
export function assetPlan(assetId = "asset-a", fields: Partial<AgentAssetPlan> = {}): AgentAssetPlan {
  return { token: `plan:${assetId}`, assetId, action: "enable", title: "启用 MCP", mechanismId: "fixture-mechanism", selectedInstallationId: "installation-a", expiresAt: new Date(Date.now() + 60_000).toISOString(), changes: [{ label: "启用目标 MCP", path: "/fixture/config/source-a.toml", before: "enabled = false", after: "enabled = true" }], affectedAssetIds: [assetId], affectedInstallationIds: ["installation-a", "installation-b"], sourceIds: ["source-a"], reloadEffect: "重启 Agent 后生效", trustEffect: null, ...fields };
}
export function assetOperation(id = "operation-a", fields: Partial<AgentAssetOperation> = {}): AgentAssetOperation {
  return { id, assetId: "asset-a", action: "enable", phase: "waitingForLock", canCancel: true, revision: 1, createdAt: "2026-09-13T00:00:00Z", updatedAt: "2026-09-13T00:00:00Z", outcome: null, message: null, affectedAssetIds: ["asset-a"], reloadEffect: null, ...fields };
}
export function assetPreview(id = "source-a", fields: Partial<AgentAssetReadResult> = {}): AgentAssetReadResult {
  return { stableId: id, accessId: `access:${id}`, sourceRevision: assetRevision, path: `/fixture/config/${id}.toml`, content: "enabled = false", sizeBytes: 15, modifiedAt: null, truncated: false, metadataOnly: false, diagnostics: [], ...fields };
}
