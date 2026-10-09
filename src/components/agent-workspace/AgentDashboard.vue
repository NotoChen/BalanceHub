<script setup lang="ts">
import { computed, nextTick, reactive, ref, toRef, watch } from "vue";
import { RefreshCw } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import { useAgentDashboard } from "../../composables/useAgentDashboard";
import { useCardDragSort } from "../../composables/useCardDragSort";
import { agentWorkspacePages, type AgentWorkspacePage } from "../../stores/agent-workspace";
import type { AgentCatalogOperation } from "../../stores/agent-catalog-types";
import type { AgentCliKind, AgentRuntimeSession, AgentRuntimeSnapshot, CliRuntimeSnapshot, Provider } from "../../stores/provider-types";
import AgentOverviewCard from "./AgentOverviewCard.vue";
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentConfigurationEditorModal from "./AgentConfigurationEditorModal.vue";
import AgentConfigurationPreviewModal from "./AgentConfigurationPreviewModal.vue";
import AgentCatalogPanel from "./AgentCatalogPanel.vue";
import AgentCatalogDetailDrawer from "./AgentCatalogDetailDrawer.vue";
import AgentCatalogDefinitionModal from "./AgentCatalogDefinitionModal.vue";
import AgentCatalogDefinitionForm from "./AgentCatalogDefinitionForm.vue";
import AgentCatalogPlanModal from "./AgentCatalogPlanModal.vue";
import AgentCatalogRelationModal from "./AgentCatalogRelationModal.vue";
import AgentInstallationModal from "./AgentInstallationModal.vue";
import AgentConfigurationFileActions from "./AgentConfigurationFileActions.vue";
import AgentOperationPanel from "./AgentOperationPanel.vue";
import AgentHookPanel from "./AgentHookPanel.vue";
import AgentLaunchProviderModal from "./AgentLaunchProviderModal.vue";
import AgentSessionPanel from "./AgentSessionPanel.vue";
import AgentWorkspaceScopeSelect from "../settings/agent-environment/AgentWorkspaceScopeSelect.vue";
import AgentAssetDetailDrawer from "../settings/agent-environment/AgentAssetDetailDrawer.vue";
import AgentAssetPlanModal from "../settings/agent-environment/AgentAssetPlanModal.vue";
import AgentAssetAccessModal from "../settings/agent-environment/AgentAssetAccessModal.vue";
import AgentAssetOperationList from "../settings/agent-environment/AgentAssetOperationList.vue";
import AgentHookPlanModal from "../settings/agent-environment/AgentHookPlanModal.vue";

const props = defineProps<{
  active: boolean; providers: Provider[]; cliRuntime: CliRuntimeSnapshot; runtimeSnapshot: AgentRuntimeSnapshot;
  runtimeLoading: boolean; activatingId: string | null;
}>();
const emit = defineEmits<{
  refreshing: [loading: boolean]; refreshRuntime: []; activateRuntime: [session: AgentRuntimeSession]; launch: [provider: Provider, kind: AgentCliKind];
}>();
const sessionPanel = ref<InstanceType<typeof AgentSessionPanel> | null>(null);
const catalogPanel = ref<InstanceType<typeof AgentCatalogPanel> | null>(null);
const content = ref<HTMLElement | null>(null);
const model = reactive(useAgentDashboard({ active: toRef(props, "active"), cliRuntime: toRef(props, "cliRuntime"), runtime: toRef(props, "runtimeSnapshot") }));
const sessionOpened = ref(model.navigation.page === "sessions");
watch(() => model.navigation.page, (page) => { if (page === "sessions") sessionOpened.value = true; });
const drag = reactive(useCardDragSort({
  items: computed(() => model.cards),
  getId: (card) => card.agent.kind,
  gridSelector: ".agent-overview-grid",
  dataId: "agentKind",
  reorder: model.reorderCards,
  onError: (error) => Message.error(error instanceof Error ? error.message : String(error)),
}));
const orderedCards = computed(() => drag.orderedGroups.get("") ?? []);
const sortAnnouncement = ref("");
function cancelCardDrag() { if (drag.state.id) drag.reset(true); }
function suppressDragClick(event: MouseEvent) {
  if (!drag.clickSuppressed) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  drag.clickSuppressed = false;
}
async function moveCard(kind: AgentCliKind, event: KeyboardEvent) {
  if (event.target !== event.currentTarget || !event.altKey) return;
  const step = ["ArrowLeft", "ArrowUp"].includes(event.key) ? -1 : ["ArrowRight", "ArrowDown"].includes(event.key) ? 1 : 0;
  if (!step) return;
  event.preventDefault();
  cancelCardDrag();
  const ids = orderedCards.value.map((card) => card.agent.kind);
  const index = ids.indexOf(kind);
  const target = index + step;
  if (index < 0 || target < 0 || target >= ids.length) return;
  ids.splice(index, 1);
  ids.splice(target, 0, kind);
  try {
    await model.reorderCards(ids);
    const label = model.cards.find((card) => card.agent.kind === kind)?.agent.label ?? "Agent";
    sortAnnouncement.value = `${label} 已移至第 ${target + 1} 位`;
  } catch (error) { Message.error(error instanceof Error ? error.message : String(error)); }
}
watch(() => [props.active, model.navigation.navigationRevision, model.navigation.query], cancelCardDrag, { flush: "sync" });
const native = model.environment.assets;
const selectedNativeOperations = computed(() => native.operations.filter((operation) => model.navigation.operationDetails?.kind === "native"
  && model.navigation.operationDetails.id === operation.id));
const taskRecoveryError = computed(() => [...new Set([model.catalogStore.recoveryError, model.lifecycleStore.recoveryError, native.operationRecoveryError].filter(Boolean))].join("；"));
const operationAssetLabels = computed(() => Object.fromEntries((model.environment.inventory?.assets ?? []).map((asset) => [asset.stableId, asset.label || asset.nativeId])));
const sourceContext = computed(() => native.selectedSource ? native.catalog.indexes.contexts.get(native.selectedSource.contextId) ?? null : null);
const sourceInstallations = computed(() => (sourceContext.value?.compatibleInstallationIds ?? []).flatMap((id) => {
  const installation = native.catalog.indexes.installations.get(id); return installation ? [installation] : [];
}));
const affectedAssets = computed(() => (native.pendingPlan?.affectedAssetIds ?? []).map((id) => ({ id, label: operationAssetLabels.value[id] || id })));
const affectedInstallations = computed(() => (native.pendingPlan?.affectedInstallationIds ?? []).map((id) => {
  const installation = native.catalog.indexes.installations.get(id); return { id, label: installation?.label || id, path: installation?.executablePath ?? null };
}));
const launchLabel = computed(() => props.cliRuntime.agents.find((agent) => agent.kind === model.launchKind)?.label || "Agent");
const operationIndex = computed(() => {
  const busy = new Set<string>();
  const latest = new Map<string, AgentCatalogOperation>();
  for (const operation of Object.values(model.catalogStore.operations)) {
    if (operation.phase !== "completed" && model.catalogStore.polling[operation.id]) busy.add(operation.assetId);
    const previous = latest.get(operation.assetId);
    if (!previous || operation.createdAt > previous.createdAt) latest.set(operation.assetId, operation);
  }
  return { busy, latest };
});
watch(() => model.loading, (loading) => emit("refreshing", loading), { immediate: true });
watch(() => model.navigation.page, () => { if (content.value) content.value.scrollTop = 0; }, { flush: "post" });
watch(() => props.active, (active) => { if (!active) { model.invalidate(); model.catalog.invalidate(); } }, { flush: "sync" });
watch(() => model.catalog.selectedId, () => native.closeDetail(), { flush: "sync" });
function launch(providerId: string) {
  const kind = model.launchKind;
  const provider = props.providers.find((item) => item.identity.id === providerId);
  model.launchKind = null;
  if (props.active && kind && provider) emit("launch", provider, kind);
}
function refresh() { if (model.navigation.page === "sessions") return sessionPanel.value?.refresh(); emit("refreshRuntime"); return model.refresh(); }
function refreshCard(kind: AgentCliKind) { emit("refreshRuntime"); return model.refreshCard(kind); }
function busyAsset(id: string) { return Boolean(model.catalog.libraryBusy[id] || model.catalogStore.starting[id] || operationIndex.value.busy.has(id)); }
function assetError(id: string) {
  const latest = operationIndex.value.latest.get(id);
  return model.catalog.libraryErrors[id] || model.catalogStore.startErrors[id] || (latest && model.catalogStore.errors[latest.id]) || "";
}
function recoverTasks() { void Promise.allSettled([model.catalogStore.recover(), model.lifecycleStore.recover(), native.recoverOperations()]); }
function openCatalogNative(id: string) { model.catalog.management.close(); model.openNative(id); }
function selectAgent(value: unknown) { model.navigation.selectAgent(props.cliRuntime.agents.find((agent) => agent.kind === value)?.kind ?? null); }
function openLibrary(id: string | null = null) {
  catalogPanel.value?.clearFilters();
  model.navigation.openLibrary(id);
  const revision = model.navigation.navigationRevision;
  if (id) void nextTick(() => {
    if (model.navigation.navigationRevision === revision) catalogPanel.value?.revealAsset(id);
  });
}
function openPage(page: AgentWorkspacePage) {
  if (page === "library") openLibrary();
  else model.navigation.openPage(page);
}
function editLibraryDefinition(id: string) {
  const asset = model.catalog.catalog?.assets.find((asset) => asset.id === id);
  if (asset?.ownership === "managed") void model.catalog.openEditor(id, asset.category);
}
function openHookInstallation(kind: AgentCliKind, id: string) {
  model.installation.open(kind, id);
}
defineExpose({ refresh });
</script>

<template>
  <main class="agent-dashboard" aria-label="Agent 工作台">
    <header class="agent-dashboard-navigation">
      <nav aria-label="Agent 管理分类"><button v-for="page in agentWorkspacePages" :key="page.value" type="button" :class="{ active: model.navigation.page === page.value }" :aria-current="model.navigation.page === page.value ? 'page' : undefined" @click="openPage(page.value)"><AgentWorkspaceIcon :page="page.value" /><span>{{ page.label }}</span></button></nav>
      <div class="agent-workspace-context">
        <a-select :model-value="model.navigation.agentFilter || ''" size="small" class="agent-workspace-agent-select" aria-label="筛选 Agent" @change="selectAgent">
          <a-option value="">全部 Agent</a-option><a-option v-for="agent in cliRuntime.agents" :key="agent.kind" :value="agent.kind"><span class="agent-workspace-agent-option"><AgentCliIcon :kind="agent.kind" :size="16" />{{ agent.label }}</span></a-option>
        </a-select>
        <div v-if="model.navigation.page !== 'sessions'" class="agent-workspace-scope"><AgentWorkspaceScopeSelect :model-value="model.navigation.workspacePath || ''" :options="model.workspaceOptions" @update:model-value="model.navigation.selectWorkspace($event || undefined)" /></div>
      </div>
    </header>
    <div ref="content" class="agent-dashboard-content" :class="{ 'has-catalog': model.navigation.page !== 'overview' && model.navigation.page !== 'sessions' }" @scroll="cancelCardDrag">
      <p v-if="taskRecoveryError" class="agent-workspace-error" role="alert">{{ taskRecoveryError }}<a-button type="text" size="small" :loading="model.catalogStore.recovering || model.lifecycleStore.recovering" @click="recoverTasks">重新读取任务状态</a-button></p>
      <template v-if="model.navigation.page === 'overview'">
        <header class="agent-section-toolbar agent-overview-toolbar"><p>{{ model.cards.length }} 个 Agent</p><a-button size="small" :loading="model.lifecycleStore.checkingVersions" @click="model.lifecycleStore.refresh('force')"><template #icon><RefreshCw :size="16" /></template>检查更新</a-button></header>
        <p v-if="model.error || model.lifecycleStore.error" class="agent-workspace-error" role="alert">{{ model.error || model.lifecycleStore.error }}</p>
        <div v-if="!model.cards.length" class="agent-workspace-empty">{{ model.navigation.query ? '没有匹配的 Agent' : '正在读取已注册 Agent' }}</div>
        <TransitionGroup v-else tag="div" name="agent-card-sort" class="agent-overview-grid"><AgentOverviewCard v-for="card in orderedCards" :key="card.agent.kind" v-bind="card" :loading="model.loading"
          class="agent-card-sortable" :class="{ 'agent-card-sort-placeholder': drag.state.dragging && drag.state.id === card.agent.kind, 'agent-card-sort-target': drag.overId === card.agent.kind }"
          tabindex="0" :aria-label="`${card.agent.label}，可拖拽排序，按 Alt 加方向键调整位置`"
          @pointerdown="drag.handlePointerDown(card, $event)" @dragstart.prevent @click.capture="suppressDragClick" @keydown="moveCard(card.agent.kind, $event)"
          @assets="model.navigation.openPage($event, card.agent.kind)" @hooks="model.navigation.openPage('hook', card.agent.kind)"
          @open-file="model.openConfigurationFile(card.agent.kind, $event)"
          @refresh="refreshCard(card.agent.kind)" @documentation="model.openDocumentation(card.agent.kind)"
          @installation="model.installation.open(card.agent.kind)" @installation-guide="model.openDocumentation(card.agent.kind, 'installation')"
          @history="model.navigation.openSessions('history', card.agent.kind, { workspaceMode: 'all' })" @runtime="model.navigation.openSessions('active', card.agent.kind)" @launch="model.launchKind = card.agent.kind" /></TransitionGroup>
        <span class="agent-card-sort-announcement" role="status">{{ sortAnnouncement }}</span>
        <Teleport to="body"><AgentOverviewCard v-if="drag.draggedItem" v-bind="drag.draggedItem" :loading="model.loading" class="agent-card-sort-preview" :style="drag.dragStyle()" aria-hidden="true" inert /></Teleport>
      </template>
      <AgentHookPanel v-else-if="model.navigation.page === 'hook'" :catalog="model.catalog.catalog" :agents="cliRuntime.agents" :agent-filter="model.navigation.agentFilter" :query="model.navigation.query" :focused-asset-id="model.focusedHookId" :center="model.environment" :hooks="model.hooks" :loading="model.loading" :error="model.error" :busy="busyAsset" :row-error="assetError" @installation="openHookInstallation" @detail="model.catalog.openDetail" @create="model.catalog.openEditor(null, $event)" @action="model.catalog.openPlan" :agent-selection="model.catalog.management.selection" :agent-panel="model.catalog.management.panel" :agent-loading="model.catalog.management.loading" :agent-error="model.catalog.management.error" @manage="model.catalog.openAgentPanel" @retry="model.catalog.management.retry" @native="openCatalogNative" />
      <AgentCatalogPanel v-else-if="model.navigation.page !== 'sessions'" ref="catalogPanel" :focused-asset-id="model.navigation.page === 'library' ? model.navigation.libraryAssetId : null" @edit="editLibraryDefinition" :catalog="model.catalog.catalog" :page="model.navigation.page" :query="model.navigation.query" :agent-filter="model.navigation.agentFilter" :agents="cliRuntime.agents" :loading="model.loading" :error="model.error" :busy="busyAsset" :row-error="assetError" @detail="model.catalog.openDetail" @create="model.catalog.openEditor(null, $event)" @action="model.catalog.openPlan" :agent-selection="model.catalog.management.selection" :agent-panel="model.catalog.management.panel" :agent-loading="model.catalog.management.loading" :agent-error="model.catalog.management.error" @manage="model.catalog.openAgentPanel" @retry="model.catalog.management.retry" @native="openCatalogNative" />
      <AgentSessionPanel v-if="sessionOpened" v-show="model.navigation.page === 'sessions'" ref="sessionPanel" :active="active" :providers="providers" :installations="model.installations" :cli-runtime="cliRuntime" :runtime-snapshot="runtimeSnapshot" :runtime-sessions="model.runtimeSessions" :runtime-loading="runtimeLoading" :activating-id="activatingId" @refresh-runtime="emit('refreshRuntime')" @activate-runtime="emit('activateRuntime', $event)" />
    </div>
    <a-drawer :visible="Boolean(model.navigation.operationDetails)" width="min(820px, calc(100vw - 32px))" :footer="false" title="任务详情" class="agent-workspace-drawer" closable mask-closable esc-to-close unmount-on-close @cancel="model.navigation.closeOperation">
      <div v-if="model.navigation.operationDetails" class="agent-modal-body">
        <AgentAssetOperationList v-if="model.navigation.operationDetails.kind === 'native'" :operations="selectedNativeOperations" :asset-labels="operationAssetLabels" :busy="native.operationBusy" :error="native.operationRecoveryError" @cancel="native.cancelOperation" @verify="native.verifyOperation" @refresh="native.recoverOperations" />
        <AgentOperationPanel v-else :selection="model.navigation.operationDetails" :selected-paths="model.installation.selectedPaths" :saving-path="model.installation.savingPath" :path-errors="model.installation.pathErrors" @use-path="model.installation.savePath" />
      </div>
    </a-drawer>
    <AgentConfigurationEditorModal :model="model.configuration.editor">
      <template #file-actions="{ sourceId }"><AgentConfigurationFileActions :source="model.configuration.editorSource(sourceId)" :busy="model.configuration.opening[sourceId]" @action="(source, action) => model.configuration.action(source.sourceId, action, source)" /></template>
    </AgentConfigurationEditorModal>
    <AgentConfigurationPreviewModal :model="model.configuration" />
    <AgentAssetAccessModal :confirmation="model.configuration.accessConfirmation" @close="model.configuration.closeAccessConfirmation" @confirm="model.configuration.confirmAccess" />
    <AgentCatalogDetailDrawer :busy="Boolean(model.resource.asset && busyAsset(model.resource.asset.id))" :visible="model.catalog.detailVisible" :asset="model.resource.asset" :catalog="model.catalog.catalog" :resource="model.resource" :agents="cliRuntime.agents" :library-busy="model.catalog.libraryBusy" :library-errors="model.catalog.libraryErrors" :focused-feature="model.catalog.selectedFeature" @close="model.resource.close" @native="model.openNative" @library="openLibrary" @adopt="model.catalog.adoptBinding" @delete="model.catalog.deleteDefinition" @relation="model.catalog.relations.open" @detail="model.catalog.openDetail" @action="(action, targets) => model.resource.asset && model.catalog.openPlan(model.resource.asset.id, action, targets)">
      <template #definition><AgentCatalogDefinitionForm v-if="model.resource.definitionVisible" :visible="model.catalog.editorVisible" :editing-asset-id="model.catalog.editorAssetId" :definition="model.catalog.editorDefinition" :initial-draft="model.catalog.editorDraft" :category="model.catalog.editorCategory" :agents="cliRuntime.agents" :preferred-agent-kind="model.resource.sharedAgentKind ?? model.navigation.agentFilter" :loading="model.catalog.editorLoading" :saving="model.catalog.editorSaving" :error="model.catalog.editorError" embedded @retry="model.resource.reloadShared" @close="model.resource.close" @save="model.resource.saveShared" @apply="model.resource.applyShared" /></template>
    </AgentCatalogDetailDrawer>
    <AgentCatalogDefinitionModal v-if="!model.catalog.detailVisible || !model.catalog.editorAssetId" :visible="model.catalog.editorVisible" :editing-asset-id="model.catalog.editorAssetId" :definition="model.catalog.editorDefinition" :initial-draft="model.catalog.editorDraft" :category="model.catalog.editorCategory" :agents="cliRuntime.agents" :preferred-agent-kind="model.navigation.agentFilter" :loading="model.catalog.editorLoading" :saving="model.catalog.editorSaving" :error="model.catalog.editorError" @close="model.catalog.closeEditor" @retry="model.catalog.retryEditor" @save="model.catalog.saveDefinition" @apply="model.catalog.saveAndApply" />
    <AgentCatalogPlanModal :can-return-to-editor="model.catalog.canReturnToEditor" :comparison-asset-id="model.catalog.comparisonAssetId" :name="model.catalog.planName" :visible="model.catalog.planVisible" :catalog="model.catalog.catalog" :action="model.catalog.planAction" :choices="model.catalog.targetChoices" :selection="model.catalog.configurationSelection" :selected="model.catalog.selectedTargets" :plan="model.catalog.pendingPlan" :preparing="model.catalog.preparing" :error="model.catalog.planError" :expired="model.catalog.planExpired" :can-confirm="model.catalog.canConfirm" @close="model.catalog.closePlan" @select="model.catalog.setTargets" @prepare="model.catalog.preparePlan" @confirm="model.catalog.confirmPlan" @compare="model.catalog.relations.open" />
    <AgentCatalogRelationModal :visible="model.catalog.relations.visible" :preview="model.catalog.relations.preview" :intent="model.catalog.relations.intent" :agents="cliRuntime.agents" :loading="model.catalog.relations.loading" :error="model.catalog.relations.error" :expired="model.catalog.relations.expired" :can-confirm="model.catalog.relations.canConfirm" :can-back="model.catalog.relations.canBack" @close="model.catalog.relations.close" @retry="model.catalog.relations.retry" @back="model.catalog.relations.back" @choose="model.catalog.relations.choose" @confirm="model.catalog.relations.confirm" />
    <AgentInstallationModal :model="model.installation" :center="model.environment" :hooks="model.hooks" @hooks="model.navigation.openPage('hook', $event)" @installation-guide="model.openDocumentation($event, 'installation')" />
    <AgentAssetDetailDrawer :visible="native.drawerVisible && !model.catalog.detailVisible" :detail="native.selectedDetail" :source="native.selectedSource" :source-context="sourceContext" :source-installations="sourceInstallations" :preview="native.preview" :preview-state="native.previewState" :preview-error="native.previewError" :copied-path-id="native.copiedPathId" :busy="native.rowBusy" :error="native.rowError" @close="native.closeDetail" @detail="model.openNative" @source="native.openSourceDetail" @action="native.action" @copy="native.copyPath" @hook="model.showHook" />
    <AgentAssetPlanModal :visible="native.planVisible" :plan="native.pendingPlan" :preparing="Boolean(native.planningAssetId)" :error="native.planError" :expired="native.planExpired" :can-apply="native.canApplyPlan" :affected-assets="affectedAssets" :affected-installations="affectedInstallations" @close="native.closePlan" @retry="native.retryPlan" @confirm="native.confirmPlan" />
    <AgentAssetAccessModal :confirmation="native.accessConfirmation" @close="native.closeAccessConfirmation" @confirm="native.confirmAccess" />
    <AgentHookPlanModal :visible="model.hooks.planVisible" :plan="model.hooks.pendingPlan" :can-apply="model.hooks.canApplyPlan" @close="model.hooks.closePlan" @confirm="model.confirmHookPlan" />
    <AgentLaunchProviderModal :visible="Boolean(model.launchKind)" :agent-label="launchLabel" :providers="providers" @close="model.launchKind = null" @select="launch" />
  </main>
</template>
