<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { agentCatalogSortOptions, useAgentCatalogView } from "../../composables/useAgentCatalogView";
import { Info, Plus, SlidersHorizontal, X } from "@lucide/vue";
import AgentCatalogRow from "./AgentCatalogRow.vue";
import AgentCatalogList from "./AgentCatalogList.vue";
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import type { AgentAssetCatalog, AgentCatalogAction, AgentCatalogAgentPanel } from "../../stores/agent-catalog-types";
import type { AgentCatalogAgentSelection } from "../../composables/useAgentCatalogAgentPanel";
import type { AgentAssetCategory, AgentCliDescriptor, AgentCliKind } from "../../stores/provider-types";
import type { AgentWorkspacePage } from "../../stores/agent-workspace";
import { agentAssetCategoryLabels, agentAssetStateLabels, agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import { agentAssetProvisionLabels, agentAssetInstallationLabels } from "../../utils/agent-asset-provenance";

const props = defineProps<{
  catalog: AgentAssetCatalog | null; page: AgentWorkspacePage; query: string; agentFilter: AgentCliKind | null;
  agents: AgentCliDescriptor[]; loading: boolean; error: string; busy: (assetId: string) => boolean;
  rowError?: (assetId: string) => string;
  focusedNativeId?: string | null;
  focusedAssetId?: string | null;
  agentSelection?: AgentCatalogAgentSelection | null; agentPanel?: AgentCatalogAgentPanel | null; agentLoading?: boolean; agentError?: string;
}>();
const emit = defineEmits<{
  detail: [id: string, feature?: string]; create: [category: AgentAssetCategory]; edit: [id: string];
  action: [id: string, action: AgentCatalogAction, targets?: string[]];
  manage: [id: string, kind: AgentCliKind | null]; retry: []; native: [id: string];
}>();
const { state, source, provision, installation, scope, sort, timeField, hasAssetFilters, hasFilters, clearFilters,
  library, libraryCategory, libraryAssets, libraryCategories, libraryCounts,
  labels, heading, createCategory, sourceOptions, rows, discoveryNotes, isFocused } = useAgentCatalogView(props);
const moreFiltersVisible = ref(false);
const list = ref<InstanceType<typeof AgentCatalogList> | null>(null);
const timeDescription = "按来源文件时间排序；多来源取最早创建、最近修改时间。共用配置文件的条目时间相同，未提供时间的条目排在末尾。";
const activeSourceFilters = computed(() => [
  ...(provision.value !== "all" ? [{ key: "provision", label: agentAssetProvisionLabels[provision.value], clear: () => { provision.value = "all"; } }] : []),
  ...(installation.value !== "all" ? [{ key: "installation", label: agentAssetInstallationLabels[installation.value], clear: () => { installation.value = "all"; } }] : []),
  ...(scope.value !== "all" ? [{ key: "scope", label: `${agentAssetScopeLabels[scope.value]}配置`, clear: () => { scope.value = "all"; } }] : []),
  ...(source.value ? [{ key: "source", label: sourceOptions.value.find((item) => item.id === source.value)?.path ?? "已选配置路径", clear: () => { source.value = ""; } }] : []),
]);
watch(() => props.page, () => { moreFiltersVisible.value = false; });
const inventoryNotes = computed(() => [...new Set([...(props.catalog?.diagnostics ?? []), ...discoveryNotes.value])]);
const hookSourceCount = computed(() => rows.value.reduce((count, asset) => count + (asset.hook?.sources.length ?? 0), 0));
const focusedAssetId = computed(() => rows.value.find(isFocused)?.id ?? null);
const pathOptions = computed(() => [{ value: "", label: "全部配置路径" }, ...sourceOptions.value.map((item) => ({ value: item.id, label: item.path }))]);
const listResetKey = computed(() => JSON.stringify([props.page, props.catalog?.inventory.workspace, props.query, props.agentFilter,
  props.focusedNativeId, props.focusedAssetId, libraryCategory.value, state.value, source.value, provision.value, installation.value, scope.value, sort.value]));
function closeAgentPanel() { if (props.agentSelection) emit("manage", props.agentSelection.assetId, null); }
watch(listResetKey, closeAgentPanel);
defineExpose({ clearFilters, revealAsset: (id: string) => list.value?.scrollToAsset(id) });
</script>

<template>
  <section class="agent-catalog-panel" :aria-label="`${heading} 管理`">
    <header v-if="library" class="agent-library-heading"><h2>共享库</h2><p>使用状态按当前配置范围显示。</p></header>
    <header class="agent-section-toolbar agent-catalog-toolbar">
      <div class="agent-catalog-filters">
      <div v-if="library" class="agent-library-categories" role="group" aria-label="共享资产类型">
        <button type="button" :aria-pressed="libraryCategory === 'all'" @click="libraryCategory = 'all'">全部 <span>{{ catalog ? libraryAssets.length : '—' }}</span></button>
        <button v-for="category in libraryCategories" :key="category" type="button" :aria-pressed="libraryCategory === category" @click="libraryCategory = category">{{ agentAssetCategoryLabels[category] }} <span>{{ libraryCounts.get(category) ?? 0 }}</span></button>
      </div>
      <template v-else>
      <a-select v-model="state" size="small" aria-label="筛选生效状态"><a-option value="all">全部生效状态</a-option><a-option v-for="(label, value) in agentAssetStateLabels" :key="value" :value="value">{{ label }}</a-option></a-select>
      <a-button type="text" size="small" :aria-expanded="moreFiltersVisible" :aria-controls="`agent-catalog-${page}-more-filters`" @click="moreFiltersVisible = !moreFiltersVisible"><template #icon><SlidersHorizontal :size="15" /></template>{{ moreFiltersVisible ? '收起筛选' : '筛选条件' }}<span v-if="activeSourceFilters.length" class="agent-catalog-filter-count">{{ activeSourceFilters.length }}</span></a-button>
      <a-button v-if="hasAssetFilters" type="text" size="small" @click="clearFilters">重置筛选</a-button>
      </template>
      <span v-if="!library || query.trim() || agentFilter" class="agent-catalog-count"><strong>{{ catalog ? rows.length : '—' }}</strong>{{ page === 'hook' ? '条规则' : '项' }}<template v-if="page === 'hook' && hookSourceCount"> · {{ hookSourceCount }} 个来源</template></span>
      </div>
      <div class="agent-catalog-list-actions">
        <a-tooltip v-if="timeField" :content="timeDescription" :trigger="['hover', 'focus']"><span class="agent-catalog-sort-help" tabindex="0" role="img" :aria-label="timeDescription"><Info :size="15" aria-hidden="true" /></span></a-tooltip>
        <a-select v-model="sort" size="small" class="agent-catalog-sort" aria-label="资源排序"><a-option v-for="option in agentCatalogSortOptions" :key="option.value" :value="option.value">{{ option.label }}</a-option></a-select>
        <a-button v-if="createCategory" type="primary" size="small" @click="emit('create', createCategory)"><template #icon><Plus :size="14" /></template>新建 {{ agentAssetCategoryLabels[createCategory] }}</a-button>
        <a-dropdown v-else-if="library && catalog?.creatableCategories.length" trigger="click">
          <a-button type="primary" size="small"><template #icon><Plus :size="14" /></template>新建共享资产</a-button>
          <template #content><a-doption v-for="category in catalog.creatableCategories" :key="category" @click="emit('create', category)">新建 {{ agentAssetCategoryLabels[category] }}</a-doption></template>
        </a-dropdown>
      </div>
    </header>
    <div v-if="!library && moreFiltersVisible" :id="`agent-catalog-${page}-more-filters`" class="agent-catalog-filters agent-catalog-more-filters">
      <a-select v-model="provision" size="small" aria-label="筛选提供方式"><a-option value="all">全部提供方式</a-option><a-option v-for="(label, value) in agentAssetProvisionLabels" :key="value" :value="value">{{ label }}</a-option></a-select>
      <a-select v-model="installation" size="small" aria-label="筛选安装来源"><a-option value="all">全部安装来源</a-option><a-option v-for="(label, value) in agentAssetInstallationLabels" :key="value" :value="value">{{ label }}</a-option></a-select>
      <a-select v-model="scope" size="small" aria-label="筛选配置范围"><a-option value="all">全部配置范围</a-option><a-option v-for="(label, value) in agentAssetScopeLabels" :key="value" :value="value">{{ label }}</a-option></a-select>
      <a-select v-model="source" size="small" class="agent-catalog-path-filter" aria-label="筛选配置路径" allow-search :options="pathOptions" :virtual-list-props="{ height: 256 }" />
    </div>
    <div v-if="!library && !moreFiltersVisible && activeSourceFilters.length" class="agent-catalog-active-filters" aria-label="已启用的来源筛选">
      <button v-for="filter in activeSourceFilters" :key="filter.key" type="button" :title="filter.label" :aria-label="`清除筛选：${filter.label}`" @click="filter.clear"><span>{{ filter.label }}</span><X :size="12" aria-hidden="true" /></button>
    </div>
    <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
    <details v-if="inventoryNotes.length" class="agent-catalog-notes" aria-label="扫描说明"><summary>扫描说明 · {{ inventoryNotes.length }}</summary><div class="agent-workspace-note"><p v-for="note in inventoryNotes" :key="note">{{ note }}</p></div></details>
    <div v-if="loading && !catalog" class="agent-workspace-empty">正在读取 {{ heading }}…</div>
    <div v-else-if="!rows.length" class="agent-workspace-empty agent-catalog-empty">
      <AgentWorkspaceIcon :page="page" :size="28" />
      <strong>{{ query.trim() || hasFilters ? `没有匹配的 ${heading}` : library ? '共享库中还没有资产' : discoveryNotes.length ? '暂未读取到内容，请展开扫描说明查看原因' : `当前范围暂无 ${heading}` }}</strong>
      <p v-if="query.trim() || hasFilters">可调整顶部搜索、Agent 或筛选条件。</p>
      <p v-else-if="library">可新建共享资产，或在已有资产的“配置来源与管理”中选择“保存到共享库”。</p>
      <a-button v-if="hasAssetFilters" size="small" @click="clearFilters">清除列表筛选</a-button>
    </div>
    <AgentCatalogList v-else ref="list" :assets="rows" :revision="`${page}:${catalog?.revision ?? ''}`" :reset-key="listResetKey" :focused-id="focusedAssetId" :estimated-size="(page === 'hook' ? 118 : 80) + (timeField ? 20 : 0)" @scroll="closeAgentPanel">
      <template #default="{ asset, index }">
        <AgentCatalogRow :key="asset.id" :aria-posinset="index + 1" :aria-setsize="rows.length" :asset="asset" :library="library" :labels="labels" :agent-filter="agentFilter" :busy="busy(asset.id)" :error="rowError?.(asset.id)" :focused="isFocused(asset)" :time-field="timeField" :agent-selection="agentSelection" :agent-panel="agentPanel" :agent-loading="agentLoading" :agent-error="agentError" @edit="emit('edit', $event)" @detail="(id, feature) => emit('detail', id, feature)" @action="(id, action, targets) => emit('action', id, action, targets)" @manage="(id, kind) => emit('manage', id, kind)" @retry="emit('retry')" @native="emit('native', $event)" />
      </template>
    </AgentCatalogList>
  </section>
</template>
