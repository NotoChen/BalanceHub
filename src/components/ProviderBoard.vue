<script setup lang="ts">
import { computed, ref, watch, type CSSProperties } from "vue";
import { SearchX, ServerPlus } from "@lucide/vue";
import ProviderCard from "./ProviderCard.vue";
import ProviderBoardToolbar from "./ProviderBoardToolbar.vue";
import type {
  CliRuntimeSnapshot,
  AgentRuntimeSnapshot,
  AgentCliKind,
  Provider,
  ProviderApiKeyOption,
} from "../stores/providers";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import type { ProviderCardTone } from "../utils/provider-display";
import {
  providerCardCliOrbitSpec,
  type ProviderCardCliOrbitSpec,
} from "../utils/provider-card-cli-orbit";
import { providerApiKeyDisplayName, providerDefaultApiKeyOption } from "../utils/provider-display";
import { agentCliLabel } from "../utils/cli-environment";
import { useCliRuntimeStore } from "../stores/cli-runtime";
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
  providers: Provider[];
  searchQuery?: string;
  livenessProviders: Provider[];
  regularProviders: Provider[];
  cliRuntime: CliRuntimeSnapshot;
  agentRuntimeSnapshot: AgentRuntimeSnapshot;
  switchingCliConfig: { providerId: string; cliKind: AgentCliKind } | null;
  checkingInProviderIds: string[];
  probingCapabilitiesProviderId: string | null;
  providerDrag: ProviderDragState;
  dragOverProviderId: string | null;
  draggedProvider: Provider | null;
  dragStyle: CSSProperties;
  providerCardTone: (provider: Provider) => ProviderCardTone;
  cardStatusTooltip: (provider: Provider) => string;
  showLivenessTimeline: (provider: Provider) => boolean;
}>(), { searchQuery: "" });
const cliStore = useCliRuntimeStore();
const boardRef = ref<HTMLElement | null>(null);
const activeFilter = ref<ProviderFilter>("all");
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
  retryLoad: [];
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
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addCcSwitchConfig: [provider: Provider, target: CcSwitchAppTarget];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  remove: [provider: Provider];
  openCliInstances: [provider: Provider, cliKind: AgentCliKind];
  switchCliConfig: [provider: Provider, cliKind: AgentCliKind];
  clearSearch: [];
}>();

const unfilteredSections = computed(() => [
  { key: "liveness", label: "自动测活", providers: props.livenessProviders, liveness: true },
  { key: "account", label: "账户认证", providers: props.regularProviders.filter((provider) => provider.auth.mode !== "apiKey"), liveness: false },
  { key: "apiKey", label: "API Key", providers: props.regularProviders.filter((provider) => provider.auth.mode === "apiKey"), liveness: false },
]);
const filterCounts = computed(() => countProviderFilters(
  unfilteredSections.value.flatMap((section) => section.providers),
));
const providerSections = computed(() => unfilteredSections.value.map((section) => ({
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

function providerCliOrbits(provider: Provider): ProviderCardCliOrbitSpec[] {
  return props.cliRuntime.configs
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
}

function providerActiveCliCounts(provider: Provider) {
  return activeAgentRuntimeSessions(props.agentRuntimeSnapshot).filter(isConfirmedAgentRuntimeSession).reduce<Partial<Record<AgentCliKind, number>>>(
    (counts, session) => {
      if (session.provider?.providerId === provider.identity.id) {
        counts[session.agentKind] = (counts[session.agentKind] || 0) + 1;
      }
      return counts;
    },
    {},
  );
}

function providerSwitchingCliKind(provider: Provider) {
  return props.switchingCliConfig?.providerId === provider.identity.id
    ? props.switchingCliConfig.cliKind
    : null;
}
</script>

<template>
  <section ref="boardRef" class="content provider-board">
    <a-spin v-if="loading && !initialized" tip="正在加载本地配置..." />

    <a-alert v-if="loadError" type="error" show-icon class="provider-load-error">
      <template #title>本地配置未加载</template>
      <div class="provider-load-error-content">
        <span>{{ loadError }}</span>
        <div class="provider-load-error-actions">
          <a-button type="primary" size="small" :loading="loading" @click="emit('retryLoad')">重新读取</a-button>
          <a-button size="small" :disabled="loading" @click="emit('importData')">从备份恢复</a-button>
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
      @select="activeFilter = $event"
      @reset="clearFilters"
    />
    <section v-for="section in providerSections" :key="section.key" class="provider-board-section">
      <div class="provider-board-section-header">
        <h2>{{ section.label }}</h2>
        <span>{{ section.providers.length }}</span>
      </div>
      <TransitionGroup name="provider-grid" tag="div" class="overview-provider-grid">
        <ProviderCard
          v-for="provider in section.providers"
          :key="provider.identity.id"
          :provider="provider"
          :tone="providerCardTone(provider)"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="section.liveness"
          :cli-orbits="providerCliOrbits(provider)"
          :active-cli-counts="providerActiveCliCounts(provider)"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
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
          @open-liveness-details="emit('openLivenessDetails', $event)"
          @open-check-in-records="emit('openCheckInRecords', $event)"
          @add-cc-switch-config="(provider, target) => emit('addCcSwitchConfig', provider, target)"
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
    </section>
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
      :tone="providerCardTone(draggedProvider)"
      :dragging="true"
      :interactive="false"
      :drag-style="dragStyle"
      :show-liveness-timeline="showLivenessTimeline(draggedProvider)"
      :cli-orbits="providerCliOrbits(draggedProvider)"
      :active-cli-counts="providerActiveCliCounts(draggedProvider)"
      aria-hidden
    />
  </section>
</template>
