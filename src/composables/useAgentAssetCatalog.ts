import { computed, watch, type Ref } from "vue";
import type {
  AgentAssetAction,
  AgentAssetActionKind,
  AgentAssetActionUnavailableReason,
  AgentAssetCategory,
  AgentAssetDeclaration,
  AgentAssetDetails,
  AgentAssetMechanismRecord,
  AgentAssetRecord,
  AgentAssetResolutionRelation,
  AgentAssetScope,
  AgentAssetSource,
  AgentAssetState,
  AgentConfigurationContext,
  AgentEnvironmentInventory,
  AgentInstallation,
  AgentTrustState,
} from "../stores/provider-types";
import { formatAgentAssetDiagnostics } from "../utils/agent-environment-diagnostics";

export const agentAssetCategoryLabels: Record<AgentAssetCategory, string> = {
  skill: "Skill", mcp: "MCP", plugin: "Plugin", extension: "Extension", hook: "Hook", statusUi: "Status UI",
};
export const agentAssetStateLabels: Record<AgentAssetState, string> = {
  enabled: "已启用", disabled: "已停用", notInstalled: "未安装", shadowed: "已被替换", blocked: "被阻止", invalid: "无效", unknown: "状态待核对",
};
export const agentAssetScopeLabels: Record<AgentAssetScope, string> = {
  user: "用户", workspace: "工作区", local: "本机", system: "系统", managed: "托管",
};
export const agentAssetTrustLabels: Record<AgentTrustState, string> = {
  trusted: "已信任", untrusted: "未信任", required: "需要信任", unknown: "信任未知",
};
export const agentAssetActionLabels: Record<AgentAssetActionKind, string> = {
  inspect: "详情", enable: "启用", disable: "停用", remove: "移除", preview: "预览", open: "打开", reveal: "在文件管理器中显示",
};
const relationLabels: Record<AgentAssetResolutionRelation, string> = {
  independent: "独立", replaceWinner: "替换生效", replaced: "已被替换", merged: "多源合并", additive: "并存", unknown: "关系未知",
};
const unavailableLabels: Record<AgentAssetActionUnavailableReason, string> = {
  mutationDisabled: "当前机制未开放修改",
  noOfficialMechanism: "此 Agent 尚未提供可用的原生启停方式",
  unsupportedPlatform: "当前平台不支持此操作",
  unsupportedScope: "此配置范围不支持修改",
  unsupportedSchema: "当前配置格式无法安全修改",
  noCompatibleInstallation: "没有兼容的 Agent 安装",
  installationUnavailable: "执行此操作的安装不可用",
  assetNotInstalled: "资产尚未安装，请先在 Agent 中安装",
  assetInstallationUnknown: "无法确认资产安装状态",
  ambiguousMechanism: "无法唯一确定原生操作机制",
  nativeInteractiveOnly: "请在 Agent 原生交互界面中操作",
  invocationPolicyOnly: "仅支持调用策略，不能完整启停",
  noReversibleMechanism: "尚无可逆的原生启停机制",
  managedByAssetCatalog: "请在全局资产详情中管理此规则",
  trustRequired: "需要先在 Agent 中确认信任",
  scopeAmbiguous: "无法确定操作的配置范围",
  childOwnedByParent: "由父扩展控制，请查看操作归属",
  shadowed: "此声明已被替换，不能独立修改",
  policyBlocked: "受策略限制，无法修改",
  sourceUnavailable: "配置来源不可访问",
  unknown: "当前操作不可用",
};

export function agentAssetActionReason(action: AgentAssetAction | undefined) {
  if (!action) return "当前未提供此操作";
  if (action.action === "remove" && action.reason === "noOfficialMechanism") {
    return "该 Agent 尚未提供可安全调用的原生卸载机制，请使用 Agent 的原生卸载入口";
  }
  return action.reason ? unavailableLabels[action.reason] : action.available ? "" : "当前操作不可用";
}

export interface AgentAssetRowView {
  asset: AgentAssetRecord;
  agentLabel: string;
  context: AgentConfigurationContext | null;
  sources: AgentAssetSource[];
  relations: string[];
  diagnostics: string[];
  readOnlyReason: string | null;
  providerLabel: string | null;
  actionOwnerLabel: string | null;
}

export interface AgentAssetReferenceView { id: string; label: string; missing: boolean }
export interface AgentAssetDeclarationView {
  declaration: AgentAssetDeclaration;
  source: AgentAssetSource | null;
  represented: boolean;
  contributor: boolean;
  diagnostics: string[];
}
export interface AgentAssetDetailView {
  row: AgentAssetRowView;
  declarations: AgentAssetDeclarationView[];
  provider: AgentAssetReferenceView | null;
  actionOwner: AgentAssetReferenceView | null;
  winner: AgentAssetReferenceView | null;
  children: AgentAssetReferenceView[];
  affected: AgentAssetReferenceView[];
  compatibleInstallations: AgentInstallation[];
  selectedInstallation: AgentInstallation | null;
  mechanisms: AgentAssetMechanismRecord[];
  facts: { label: string; value: string }[];
  diagnostics: string[];
}

function unique(values: string[]) { return [...new Set(values)]; }

function detailsFacts(details: AgentAssetDetails): { label: string; value: string }[] {
  const declared = { enabled: "启用", disabled: "停用", pending: "待确认", rejected: "已拒绝", unknown: "未知" };
  switch (details.kind) {
    case "skill":
      return [
        { label: "原生声明", value: declared[details.enabled] },
        { label: "调用策略", value: { modelInvocable: "允许模型调用", manualOnly: "仅手动调用", disabled: "禁止调用", unknown: "未知" }[details.invocationPolicy] },
      ];
    case "mcp":
      return [
        { label: "传输方式", value: details.transport },
        { label: "原生声明", value: declared[details.declaredState] },
        { label: "审批状态", value: { approved: "已批准", rejected: "已拒绝", pending: "待批准", notRequired: "无需批准", unknown: "未知" }[details.approvalState] },
        { label: "有效可用性", value: { available: "可用", disabled: "已停用", approvalRequired: "需要批准", policyBlocked: "被策略阻止", trustRequired: "需要信任", invalid: "无效", unknown: "未知" }[details.effectiveAvailability] },
      ];
    case "plugin":
    case "extension":
      return [
        { label: "安装状态", value: { installed: "已安装", notInstalled: "未安装", unknown: "未知" }[details.installState] },
        { label: "原生声明", value: declared[details.enabled] },
        { label: "扩展信任", value: agentAssetTrustLabels[details.trusted] },
      ];
    case "hook":
      return [{ label: "已配置规则", value: details.ruleCount === null ? "数量未确认" : `${details.ruleCount} 条` }];
    case "statusUi":
      return [
        { label: "显示模式", value: { builtIn: "内置", command: "命令", disabled: "停用", unknown: "未知" }[details.mode] },
        { label: "配置命令", value: details.commandPresent ? "存在（盘点不会执行）" : "未配置" },
      ];
  }
}

export function useAgentAssetCatalog(inventory: Ref<AgentEnvironmentInventory | null>) {
  const rowCache = new Map<string, AgentAssetRowView>();
  watch(inventory, () => rowCache.clear(), { flush: "sync" });
  const indexes = computed(() => ({
    assets: new Map((inventory.value?.assets ?? []).map((item) => [item.stableId, item])),
    contexts: new Map((inventory.value?.contexts ?? []).map((item) => [item.id, item])),
    sources: new Map((inventory.value?.sources ?? []).map((item) => [item.id, item])),
    declarations: new Map((inventory.value?.declarations ?? []).map((item) => [item.id, item])),
    installations: new Map((inventory.value?.installations ?? []).map((item) => [item.id, item])),
    mechanisms: new Map((inventory.value?.mechanisms ?? []).map((item) => [item.id, item])),
  }));

  function agentLabel(kind: AgentAssetRecord["agentKind"]) {
    return inventory.value?.installations.find((item) => item.agentKind === kind)?.label ?? kind;
  }

  function reference(id: string | null): AgentAssetReferenceView | null {
    if (!id) return null;
    const asset = indexes.value.assets.get(id);
    return { id, label: asset?.label || asset?.nativeId || id, missing: !asset };
  }

  function rowFor(assetId: string): AgentAssetRowView | null {
    const asset = indexes.value.assets.get(assetId);
    if (!asset) return null;
    const cached = rowCache.get(assetId);
    if (cached) return cached;
    const context = indexes.value.contexts.get(asset.contextId) ?? null;
    const sources = asset.sourceIds.flatMap((id) => {
      const source = indexes.value.sources.get(id);
      return source ? [source] : [];
    });
    const declarations = unique([...asset.representedDeclarationIds, ...asset.resolution.contributorIds])
      .flatMap((id) => {
        const declaration = indexes.value.declarations.get(id);
        return declaration ? [declaration] : [];
      });
    const diagnostics = unique([
      ...formatAgentAssetDiagnostics(asset.diagnostics),
      ...formatAgentAssetDiagnostics(asset.resolution.diagnostics),
      ...sources.flatMap((source) => formatAgentAssetDiagnostics(source.diagnostics)),
      ...declarations.flatMap((declaration) => formatAgentAssetDiagnostics(declaration.diagnostics)),
      ...formatAgentAssetDiagnostics((inventory.value?.diagnostics ?? []).filter((diagnostic) =>
        ("sourceId" in diagnostic && asset.sourceIds.includes(diagnostic.sourceId))
        || ("installationId" in diagnostic && asset.compatibleInstallationIds.includes(diagnostic.installationId)),
      )),
      ...(!context ? ["配置上下文引用缺失，请刷新盘点"] : []),
      ...asset.sourceIds.filter((id) => !indexes.value.sources.has(id)).map(() => "配置来源引用缺失，请刷新盘点"),
    ]);
    const relations = [relationLabels[asset.resolution.relation]];
    if (asset.resolution.qualifiedCollision) relations.push("限定名称冲突");
    if (asset.resolution.terminal) relations.push(asset.resolution.terminal === "policyBlocked" ? "策略阻止" : "终态未知");
    const mutationActions = asset.actions.filter((action) => action.action === "enable" || action.action === "disable");
    const readOnlyReason = mutationActions.some((action) => action.available)
      ? null : unique(mutationActions.map(agentAssetActionReason).filter(Boolean)).join("；") || "只读";
    const label = agentLabel(asset.agentKind);
    const row = {
      asset, agentLabel: label, context, sources, diagnostics, relations, readOnlyReason,
      providerLabel: reference(asset.relationships.providedBy)?.label ?? null,
      actionOwnerLabel: reference(asset.relationships.actionOwner)?.label ?? null,
    };
    rowCache.set(assetId, row);
    return row;
  }
  const rows = computed<AgentAssetRowView[]>(() => (inventory.value?.assets ?? []).flatMap((asset) => {
    const row = rowFor(asset.stableId);
    return row ? [row] : [];
  }));

  function detailFor(assetId: string): AgentAssetDetailView | null {
    const row = rowFor(assetId);
    if (!row) return null;
    const asset = row.asset;
    const declarationIds = unique([...asset.representedDeclarationIds, ...asset.resolution.contributorIds]);
    const missing = declarationIds.filter((id) => !indexes.value.declarations.has(id));
    const declarations = declarationIds.flatMap((id) => {
      const declaration = indexes.value.declarations.get(id);
      return declaration ? [{
        declaration, source: indexes.value.sources.get(declaration.sourceId) ?? null,
        represented: asset.representedDeclarationIds.includes(id), contributor: asset.resolution.contributorIds.includes(id),
        diagnostics: formatAgentAssetDiagnostics(declaration.diagnostics),
      }] : [];
    });
    const provider = reference(asset.relationships.providedBy);
    const actionOwner = reference(asset.relationships.actionOwner);
    const winner = reference(asset.resolution.winnerId);
    const children = (inventory.value?.assets ?? []).filter((child) => child.relationships.providedBy === asset.stableId)
      .map((child) => reference(child.stableId)!);
    const affected = asset.relationships.affectedAssetIds.map((id) => reference(id)!);
    const mechanismIds = unique(asset.actions.flatMap((action) => action.mechanismId ? [action.mechanismId] : []));
    return {
      row, declarations, provider, actionOwner, winner, children, affected,
      compatibleInstallations: asset.compatibleInstallationIds.flatMap((id) => {
        const installation = indexes.value.installations.get(id);
        return installation ? [installation] : [];
      }),
      selectedInstallation: asset.selectedActionInstallationId ? indexes.value.installations.get(asset.selectedActionInstallationId) ?? null : null,
      mechanisms: mechanismIds.flatMap((id) => {
        const mechanism = indexes.value.mechanisms.get(id);
        return mechanism ? [mechanism] : [];
      }),
      facts: detailsFacts(asset.details),
      diagnostics: unique([...row.diagnostics,
        ...(missing.length ? [`${missing.length} 条声明引用缺失，请刷新盘点`] : []),
        ...[provider, actionOwner, winner, ...children, ...affected].filter((item) => item?.missing).map((item) => `关联资产未在当前盘点中找到：${item!.label}`),
      ]),
    };
  }

  return { indexes, rows, rowFor, detailFor };
}
