<script setup lang="ts">
import { computed, defineAsyncComponent, ref, watch, type CSSProperties } from "vue";
import { storeToRefs } from "pinia";
import { SearchX, ServerPlus } from "@lucide/vue";
import ProviderCard from "./ProviderCard.vue";
import ProviderBoardToolbar from "./ProviderBoardToolbar.vue";
import { useFirstOpenLatch } from "../composables/useFirstOpenLatch";
import ProviderSectionCards from "./ProviderSectionCards.vue";
import type {
  CliRuntimeSnapshot,
  AgentRuntimeSnapshot,
  AgentCliKind,
  Provider,
  ProviderApiKeyOption,
} from "../stores/providers";
import type { ProviderAppImportTarget } from "../stores/provider-types";
import type { ProviderCardTone } from "../utils/provider-display";
import {
  providerCardCliOrbitSpec,
  type ProviderCardCliOrbitSpec,
} from "../utils/provider-card-cli-orbit";
import { providerApiKeyDisplayName, providerDefaultApiKeyOption } from "../utils/provider-display";
import { agentCliLabel } from "../utils/cli-environment";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useWorkspaceLayoutStore } from "../stores/workspace-layout";
import type { LivenessSettings } from "../utils/provider-liveness";
import { activeAgentRuntimeSessions, isConfirmedAgentRuntimeSession } from "../utils/agent-runtime";
import { countProviderFilters, providerMatchesFilter, providerFilters, type ProviderFilter } from "../utils/provider-filters";

interface ProviderDragState {
  providerId: string | null;
  dragging: boolean;
}

const props = withDefaults(defineProps<{
  loading: boolean;
  initialized: boolean;
  loadError: string | null;
  /// 运行期存储保护态：卡片仍可查看，但保存已暂停，只能重启恢复。
  storageBlockedError?: string | null;
  restoringBackup?: boolean;
  providers: Provider[];
  /// 测活目标按全局设置回落，卡片和看板使用同一份目标。
  settings?: LivenessSettings;
  searchQuery?: string;
  /// Object-type sections in drag order. Liveness and protocol are card
  /// badges, so they never move a card between grids.
  sections: { key: string; label: string; providers: Provider[] }[];
  cliRuntime: CliRuntimeSnapshot;
  agentRuntimeSnapshot: AgentRuntimeSnapshot;
  switchingCliConfig: { providerId: string; cliKind: AgentCliKind } | null;
  checkingInProviderIds: string[];
  probingCapabilitiesProviderId: string | null;
  providerDrag: ProviderDragState;
  refreshInProgress?: boolean;
  /// 仍在批次队列里的站点；已结束的卡片不再显示「排队中」。
  refreshQueuedProviderIds?: ReadonlySet<string>;
  dragOverProviderId: string | null;
  draggedProvider: Provider | null;
  dragStyle: CSSProperties;
  providerCardTone: (provider: Provider) => ProviderCardTone;
  cardStatusTooltip: (provider: Provider) => string;
  showLivenessTimeline: (provider: Provider) => boolean;
}>(), {
  searchQuery: "",
  storageBlockedError: null,
  restoringBackup: false,
  refreshInProgress: false,
  refreshQueuedProviderIds: () => new Set<string>(),
});
const cliStore = useCliRuntimeStore();
const layoutStore = useWorkspaceLayoutStore();
const { layout } = storeToRefs(layoutStore);
const boardRef = ref<HTMLElement | null>(null);
const activeFilter = ref<ProviderFilter>("all");
const recommendationVisible = ref(false);
// 推荐窗口只在用户主动打开时才有用，首次打开后再保持挂载。
const ProviderRecommendationModal = defineAsyncComponent(() => import("./ProviderRecommendationModal.vue"));
const recommendationOpened = useFirstOpenLatch(() => recommendationVisible.value);
const hasSearch = computed(() => Boolean(props.searchQuery.trim()));
const selectedFilterLabel = computed(() =>
  providerFilters.find((option) => option.value === activeFilter.value)!.label,
);
watch([() => props.searchQuery, activeFilter], () => {
  if (boardRef.value) boardRef.value.scrollTop = 0;
}, { flush: "post" });

const emit = defineEmits<{
  add: [];
  importData: [];
  restoreBackup: [];
  retryLoad: [];
  restartApp: [];
  cardClick: [provider: Provider];
  cardPointerdown: [provider: Provider, event: PointerEvent];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  probeCapabilities: [provider: Provider];
  launchTemporaryCli: [provider: Provider, cliKind?: AgentCliKind];
  edit: [provider: Provider];
  checkIn: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  selectApiKey: [provider: Provider, option: ProviderApiKeyOption];
  openAvailableModels: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  openAccountProfile: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addProviderToApp: [provider: Provider, target: ProviderAppImportTarget];
  cpaRenewAll: [provider: Provider];
  reauthorizeCard: [provider: Provider];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  remove: [provider: Provider];
  openCliInstances: [provider: Provider, cliKind: AgentCliKind];
  switchCliConfig: [provider: Provider, cliKind: AgentCliKind, model?: string];
  clearSearch: [];
}>();

const filterCounts = computed(() => countProviderFilters(
  props.sections.flatMap((section) => section.providers),
));
const providerSections = computed(() => props.sections.map((section) => ({
  ...section,
  providers: section.providers.filter((provider) => providerMatchesFilter(provider, activeFilter.value)),
})).filter((section) => section.providers.length > 0));
const visibleProviderCount = computed(() =>
  providerSections.value.reduce((total, section) => total + section.providers.length, 0),
);

function clearFilters() {
  activeFilter.value = "all";
  emit("clearSearch");
}

/// 卡片按中转站对象挂缓存：后台快照里内容没变的中转站会复用旧对象，
/// 这里返回同一份数组/计数，子组件的 props 身份不变，整排卡片才会跳过重渲染。
const providerOrbitCache = new WeakMap<Provider, {
  cliRuntime: CliRuntimeSnapshot;
  probe: unknown;
  orbits: ProviderCardCliOrbitSpec[];
}>();
const EMPTY_CLI_COUNTS: Partial<Record<AgentCliKind, number>> = Object.freeze({});
let activeCliCountsSnapshot: AgentRuntimeSnapshot | null = null;
let activeCliCountsByProvider = new Map<string, Partial<Record<AgentCliKind, number>>>();

function providerCliOrbits(provider: Provider): ProviderCardCliOrbitSpec[] {
  const probe = cliStore.cliEnvironmentProbe;
  const cached = providerOrbitCache.get(provider);
  if (cached && cached.cliRuntime === props.cliRuntime && cached.probe === probe) {
    return cached.orbits;
  }
  const orbits = props.cliRuntime.configs
    .filter((snapshot) => snapshot.providerId === provider.identity.id)
    .map((snapshot) => {
      const localId = snapshot.apiKeyLocalId?.trim() || "";
      const option = localId
        ? provider.auth.apiKeyOptions.find((item) => item.localId.trim() === localId)
        : providerDefaultApiKeyOption(provider);
      const keyLabel = option ? providerApiKeyDisplayName(option) : "当前调用 Key";
      return providerCardCliOrbitSpec(snapshot.cliKind, {
        title: `${agentCliLabel(cliStore.cliEnvironmentProbe, snapshot.cliKind)} 默认：${keyLabel}`,
      });
    });
  providerOrbitCache.set(provider, { cliRuntime: props.cliRuntime, probe, orbits });
  return orbits;
}

// 快照变化时只在整份会话列表上扫一遍，按站点聚合成一次索引；卡片渲染
// 只做一次 Map 查询，不再为每张卡片重复过滤全部会话。
function activeCliCountsIndex(snapshot: AgentRuntimeSnapshot) {
  if (activeCliCountsSnapshot === snapshot) return activeCliCountsByProvider;
  const index = new Map<string, Partial<Record<AgentCliKind, number>>>();
  for (const session of activeAgentRuntimeSessions(snapshot)) {
    if (!isConfirmedAgentRuntimeSession(session)) continue;
    const providerId = session.provider?.providerId;
    if (!providerId) continue;
    const counts = index.get(providerId) ?? {};
    counts[session.agentKind] = (counts[session.agentKind] || 0) + 1;
    index.set(providerId, counts);
  }
  activeCliCountsSnapshot = snapshot;
  activeCliCountsByProvider = index;
  return index;
}

function providerActiveCliCounts(provider: Provider) {
  return activeCliCountsIndex(props.agentRuntimeSnapshot).get(provider.identity.id) ?? EMPTY_CLI_COUNTS;
}

function providerSwitchingCliKind(provider: Provider) {
  return props.switchingCliConfig?.providerId === provider.identity.id
    ? props.switchingCliConfig.cliKind
    : null;
}
</script>

<template>
  <section
    ref="boardRef"
    class="content provider-board"
    :class="{ 'is-list': layout === 'list' }"
  >
    <a-spin v-if="loading && !initialized" tip="正在加载本地配置..." />

    <a-alert v-if="loadError && !storageBlockedError" type="error" show-icon class="provider-load-error">
      <template #title>本地配置未加载</template>
      <div class="provider-load-error-content">
        <span>{{ loadError }}</span>
        <div class="provider-load-error-actions">
          <a-button type="primary" size="small" :loading="loading" @click="emit('retryLoad')">重新读取</a-button>
          <a-button size="small" :loading="restoringBackup" :disabled="loading" @click="emit('restoreBackup')">自动备份恢复</a-button>
          <a-button size="small" :disabled="loading || restoringBackup" @click="emit('importData')">从文件恢复</a-button>
          <a-button size="small" :disabled="loading || restoringBackup" @click="emit('restartApp')">重启应用</a-button>
        </div>
      </div>
    </a-alert>

    <a-alert v-if="storageBlockedError" type="warning" show-icon class="provider-load-error">
      <template #title>本地配置已暂停写入</template>
      <div class="provider-load-error-content">
        <span>{{ storageBlockedError }}</span>
        <div class="provider-load-error-actions">
          <a-button type="primary" size="small" @click="emit('restartApp')">重启应用恢复</a-button>
        </div>
      </div>
    </a-alert>

    <template v-if="!loadError">
    <ProviderBoardToolbar
      v-if="providers.length > 0"
      :filter="activeFilter"
      :counts="filterCounts"
      :visible-count="visibleProviderCount"
      :total-count="providers.length"
      :has-search="hasSearch"
      :layout="layout"
      @select="activeFilter = $event"
      @update:layout="layoutStore.setLayout"
      @reset="clearFilters"
      @recommend="recommendationVisible = true"
    />
    <ProviderSectionCards
      v-for="section in providerSections"
      :key="section.key"
      :label="section.label"
      :count="section.providers.length"
    >
      <TransitionGroup
        name="provider-grid"
        tag="div"
        class="overview-provider-grid"
        :class="{ 'is-list': layout === 'list' }"
      >
        <ProviderCard
          v-for="provider in section.providers"
          :key="provider.identity.id"
          :provider="provider"
          :settings="settings"
          :tone="providerCardTone(provider)"
          :list="layout === 'list'"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="showLivenessTimeline(provider)"
          :cli-orbits="providerCliOrbits(provider)"
          :active-cli-counts="providerActiveCliCounts(provider)"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          :refresh-queued="refreshInProgress && refreshQueuedProviderIds.has(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="(provider, cliKind) => emit('openCliInstances', provider, cliKind)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
          @select-api-key="(provider, option) => emit('selectApiKey', provider, option)"
          @open-available-models="emit('openAvailableModels', $event)"
          @open-usage="emit('openUsage', $event)"
          @open-request-logs="emit('openRequestLogs', $event)"
          @open-password-change="emit('openPasswordChange', $event)"
          @open-account-profile="emit('openAccountProfile', $event)"
          @open-liveness-details="emit('openLivenessDetails', $event)"
          @open-check-in-records="emit('openCheckInRecords', $event)"
          @add-provider-to-app="(provider, target) => emit('addProviderToApp', provider, target)"
          @cpa-renew-all="emit('cpaRenewAll', $event)"
          @reauthorize-card="emit('reauthorizeCard', $event)"
          @launch-temporary-cli="emit('launchTemporaryCli', $event)"
          @copy-url="emit('copyUrl', $event)"
          @copy-invite="emit('copyInvite', $event)"
          @copy-secret="(provider, field) => emit('copySecret', provider, field)"
          @edit="emit('edit', $event)"
          @toggle="emit('toggle', $event)"
          @refresh="emit('refresh', $event)"
          @check-in="emit('checkIn', $event)"
          @remove="emit('remove', $event)"
        />
      </TransitionGroup>
      </ProviderSectionCards>
    </template>

    <div v-if="!loadError && providers.length === 0 && !loading" class="empty-state provider-board-empty">
      <span class="provider-board-empty-icon"><ServerPlus :size="24" aria-hidden="true" /></span>
      <h3>还没有中转站</h3>
      <p>添加中转站统一查看余额、签到和模型，也可以从已有备份导入。</p>
      <div class="provider-board-empty-actions">
        <a-button type="primary" @click="emit('add')">添加中转站</a-button>
        <a-button @click="emit('importData')">从备份导入</a-button>
      </div>
    </div>

    <div
      v-else-if="!loadError && providers.length > 0 && visibleProviderCount === 0"
      class="empty-state provider-board-empty provider-board-search-empty"
    >
      <span class="provider-board-empty-icon"><SearchX :size="24" aria-hidden="true" /></span>
      <h3>{{ hasSearch || activeFilter === 'all' ? '没有匹配的中转站' : `暂无${selectedFilterLabel}的中转站` }}</h3>
      <p>可以调整筛选条件或搜索词，也可以重置筛选查看全部中转站。</p>
      <a-button @click="clearFilters">查看全部中转站</a-button>
    </div>

    <ProviderCard
      v-if="draggedProvider"
      :provider="draggedProvider"
      :settings="settings"
      :tone="providerCardTone(draggedProvider)"
      :list="layout === 'list'"
      :dragging="true"
      :interactive="false"
      :drag-style="dragStyle"
      :show-liveness-timeline="showLivenessTimeline(draggedProvider)"
      :cli-orbits="providerCliOrbits(draggedProvider)"
      :active-cli-counts="providerActiveCliCounts(draggedProvider)"
      aria-hidden
    />

    <ProviderRecommendationModal
      v-if="recommendationOpened"
      :visible="recommendationVisible"
      :providers="providers"
      @close="recommendationVisible = false"
      @switch="(provider, cliKind, model) => emit('switchCliConfig', provider, cliKind, model)"
    />
  </section>
</template>
