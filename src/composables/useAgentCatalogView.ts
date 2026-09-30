import { computed, ref, watch } from "vue";
import type { AgentAssetCatalog, AgentCatalogAsset, AgentCatalogBinding, AgentCatalogUnresolvedTarget } from "../stores/agent-catalog-types";
import type {
  AgentAssetState, AgentCliDescriptor, AgentCliKind, AgentAssetProvision,
  AgentAssetInstallationOrigin, AgentAssetScope, AgentAssetCategory,
} from "../stores/provider-types";
import type { AgentWorkspacePage } from "../stores/agent-workspace";
import {
  agentCatalogBindingMatches, agentCatalogCategoryMatches, agentCatalogDefinitionMatches,
  agentCatalogUnresolvedMatches,
} from "../utils/agent-catalog-display";
import { agentAssetProvenanceEntries } from "../utils/agent-asset-provenance";
import { formatAgentAssetDiagnostic, mergeAgentAssetDiagnostics } from "../utils/agent-environment-diagnostics";

interface AgentCatalogViewProps {
  catalog: AgentAssetCatalog | null;
  page: AgentWorkspacePage;
  query: string;
  agentFilter: AgentCliKind | null;
  agents: AgentCliDescriptor[];
  focusedNativeId?: string | null;
  focusedAssetId?: string | null;
}

const nameCollator = new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" });
export const agentCatalogSortOptions = [
  { value: "nameAsc", label: "名称 · 正序", field: "name", direction: 1 },
  { value: "nameDesc", label: "名称 · 倒序", field: "name", direction: -1 },
  { value: "createdDesc", label: "创建时间 · 最新优先", field: "createdAt", direction: -1 },
  { value: "createdAsc", label: "创建时间 · 最早优先", field: "createdAt", direction: 1 },
  { value: "modifiedDesc", label: "修改时间 · 最新优先", field: "modifiedAt", direction: -1 },
  { value: "modifiedAsc", label: "修改时间 · 最早优先", field: "modifiedAt", direction: 1 },
] as const;

/** Filters backend logical identities; never combines or invents native bindings. */
export function useAgentCatalogView(props: Readonly<AgentCatalogViewProps>) {
  const sort = ref<typeof agentCatalogSortOptions[number]["value"]>("nameAsc");
  const sorting = computed(() => agentCatalogSortOptions.find((option) => option.value === sort.value) ?? agentCatalogSortOptions[0]);
  const timeField = computed(() => sorting.value.field === "name" ? null : sorting.value.field);
  const library = computed(() => props.page === "library");
  const libraryCategory = ref<AgentAssetCategory | "all">("all");
  const libraryAssets = computed(() => (props.catalog?.assets ?? []).filter((asset) => asset.ownership === "managed"));
  const libraryCategories = computed(() => [...new Set([
    ...(props.catalog?.creatableCategories ?? []), ...libraryAssets.value.map((asset) => asset.category),
  ])]);
  const libraryCounts = computed(() => new Map(libraryCategories.value.map((category) => [category,
    libraryAssets.value.filter((asset) => asset.category === category).length,
  ])));
  const state = ref<AgentAssetState | "all">("all");
  const source = ref("");
  const provision = ref<AgentAssetProvision | "all">("all");
  const installation = ref<AgentAssetInstallationOrigin | "all">("all");
  const scope = ref<AgentAssetScope | "all">("all");
  const hasProvenanceFilter = computed(() => provision.value !== "all" || installation.value !== "all");
  const hasSourceFilters = computed(() => Boolean(source.value || scope.value !== "all" || hasProvenanceFilter.value));
  const hasAssetFilters = computed(() => state.value !== "all" || hasSourceFilters.value || (library.value && libraryCategory.value !== "all"));
  const hasFilters = computed(() => Boolean(props.agentFilter || hasAssetFilters.value));
  const query = computed(() => props.query.trim());
  watch(() => props.page, clearFilters, { flush: "sync" });
  watch(() => props.catalog?.inventory.sources, (sources) => {
    if (source.value && !sources?.some((item) => item.id === source.value)) source.value = "";
  }, { flush: "sync" });
  const labels = computed(() => new Map(props.agents.map((agent) => [agent.kind, agent.label])));
  const contexts = computed(() => new Map(props.catalog?.inventory.contexts.map((context) => [context.id, context]) ?? []));
  const diagnostics = computed(() => {
    const inventory = props.catalog?.inventory;
    if (!inventory) return [];
    return mergeAgentAssetDiagnostics(
      inventory.diagnostics,
      inventory.sources.flatMap((item) => item.diagnostics),
      inventory.declarations.flatMap((item) => item.diagnostics),
      inventory.assets.flatMap((item) => [...item.diagnostics, ...item.resolution.diagnostics]),
    );
  });
  const discoveryNotes = computed(() => {
    const inventory = props.catalog?.inventory;
    if (!inventory) return [];
    const notes = diagnostics.value.flatMap((diagnostic) => {
      if (diagnostic.kind === "discoveryIncomplete") {
        return agentCatalogCategoryMatches(diagnostic.category, props.page)
          && (!props.agentFilter || diagnostic.agentKind === props.agentFilter)
          ? [`${labels.value.get(diagnostic.agentKind) || diagnostic.agentKind} · ${formatAgentAssetDiagnostic(diagnostic)}`]
          : [];
      }
      return diagnostic.kind === "truncated" || diagnostic.kind === "budgetExceeded"
        ? [formatAgentAssetDiagnostic(diagnostic)] : [];
    });
    if (props.page === "hook") {
      notes.push(...inventory.sources.filter((item) => item.categories.includes("hook")
        && (!props.agentFilter || contexts.value.get(item.contextId)?.agentKind === props.agentFilter))
        .flatMap((item) => item.diagnostics.filter((diagnostic) => diagnostic.kind !== "discoveryIncomplete")
          .map(formatAgentAssetDiagnostic)));
      notes.push(...inventory.hookRuleCounts.filter((entry) => entry.ruleCount === null
        && (!props.agentFilter || entry.agentKind === props.agentFilter))
        .map((entry) => `${labels.value.get(entry.agentKind) || entry.agentKind} · Hook 规则数量尚未确认，部分配置可能未读取。`));
    }
    return [...new Set(notes)];
  });
  const heading = computed(() => library.value ? "共享资产" : props.page === "extension" ? "插件与扩展" : props.page === "mcp" ? "MCP" : props.page === "hook" ? "Hook" : "Skill");
  const createCategory = computed(() => props.catalog?.creatableCategories.find((category) => library.value
    ? category === libraryCategory.value : agentCatalogCategoryMatches(category, props.page)) ?? null);
  const categoryAssets = computed(() => library.value
    ? libraryAssets.value.filter((asset) => libraryCategory.value === "all" || asset.category === libraryCategory.value)
    : (props.catalog?.assets ?? []).filter((asset) => agentCatalogCategoryMatches(asset.category, props.page)));
  const sourceOptions = computed(() => {
    const ids = new Set<string>();
    for (const asset of categoryAssets.value) {
      for (const binding of asset.bindings) {
        if (props.agentFilter && binding.native.agentKind !== props.agentFilter) continue;
        for (const evidence of binding.native.provenance) ids.add(evidence.sourceId);
      }
    }
    // Keep an existing selection visible until its source disappears on refresh.
    if (source.value) ids.add(source.value);
    return (props.catalog?.inventory.sources ?? []).filter((item) => ids.has(item.id));
  });
  function bindingMatches(asset: AgentCatalogAsset, binding: AgentCatalogBinding) {
    if ((props.agentFilter && binding.native.agentKind !== props.agentFilter)
      || (state.value !== "all" && binding.native.effectiveState !== state.value)) return false;
    if (!hasSourceFilters.value && !query.value) return true;
    const entries = agentAssetProvenanceEntries(binding.native.provenance, props.catalog?.inventory.sources ?? []);
    if (!entries.length) return !hasSourceFilters.value
      && agentCatalogBindingMatches(asset, binding, query.value, labels.value, null);
    return entries.some((entry) =>
      (provision.value === "all" || entry.evidence.provision === provision.value)
      && (installation.value === "all" || entry.evidence.installation === installation.value)
      && (!source.value || entry.evidence.sourceId === source.value)
      && (scope.value === "all" || entry.evidence.scope === scope.value)
      && agentCatalogBindingMatches(asset, binding, query.value, labels.value, entry));
  }
  function unresolvedMatches(asset: AgentCatalogAsset, target: AgentCatalogUnresolvedTarget) {
    return !source.value && !hasProvenanceFilter.value
      && (state.value === "all" || (state.value === "disabled" && target.state === "suspended"))
      && (!props.agentFilter || target.agentKind === props.agentFilter)
      && (scope.value === "all" || target.scope === scope.value)
      && agentCatalogUnresolvedMatches(asset, target, query.value, labels.value);
  }
  function bindingsFor(asset: AgentCatalogAsset) { return asset.bindings.filter((binding) => bindingMatches(asset, binding)); }
  function unresolvedFor(asset: AgentCatalogAsset) { return asset.unresolvedTargets.filter((target) => unresolvedMatches(asset, target)); }
  function isFocused(asset: AgentCatalogAsset) {
    return asset.id === props.focusedAssetId
      || Boolean(props.focusedNativeId && asset.bindings.some((binding) => binding.native.stableId === props.focusedNativeId));
  }
  const sortedAssets = computed(() => {
    const { field, direction } = sorting.value;
    return [...categoryAssets.value].sort((left, right) => {
      if (field === "name") return direction * nameCollator.compare(left.name, right.name) || left.id.localeCompare(right.id);
      const leftTime = left[field] ? Date.parse(left[field]) : NaN;
      const rightTime = right[field] ? Date.parse(right[field]) : NaN;
      const leftKnown = Number.isFinite(leftTime);
      const rightKnown = Number.isFinite(rightTime);
      if (leftKnown !== rightKnown) return leftKnown ? -1 : 1;
      return (leftKnown && rightKnown ? direction * (leftTime - rightTime) : 0)
        || nameCollator.compare(left.name, right.name) || left.id.localeCompare(right.id);
    });
  });
  const rows = computed(() => !hasFilters.value && !query.value ? sortedAssets.value : sortedAssets.value.filter((asset) =>
    bindingsFor(asset).length > 0 || unresolvedFor(asset).length > 0
      || (!props.agentFilter && state.value === "all" && !hasSourceFilters.value && !asset.bindings.length && !asset.unresolvedTargets.length
        && agentCatalogDefinitionMatches(asset, query.value)),
  ));
  function clearFilters() {
    libraryCategory.value = "all";
    state.value = "all";
    source.value = "";
    provision.value = "all";
    installation.value = "all";
    scope.value = "all";
  }
  return { state, source, provision, installation, scope, sort, timeField, hasAssetFilters, hasFilters, clearFilters,
    library, libraryCategory, libraryAssets, libraryCategories, libraryCounts,
    labels, heading, createCategory, sourceOptions, bindingsFor, unresolvedFor, rows, discoveryNotes, isFocused };
}
