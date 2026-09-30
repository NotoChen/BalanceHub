<script setup lang="ts">
import { computed, ref, watch, type CSSProperties } from "vue";
import AppTopbar from "./AppTopbar.vue";
import ProviderBoard from "./ProviderBoard.vue";
import AgentDashboard from "./agent-workspace/AgentDashboard.vue";
import { useAgentWorkspaceStore } from "../stores/agent-workspace";
import type {
  CliRuntimeSnapshot,
  AgentRuntimeSnapshot,
  AgentCliKind,
  AgentRuntimeSession,
  Provider,
  ProviderApiKeyOption,
} from "../stores/providers";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import type { ProviderCardTone } from "../utils/provider-display";
import { providerMatchesSearch } from "../utils/provider-filters";
import type { BackgroundTask } from "../composables/useBackgroundTaskCenter";

interface ProviderDragState {
  providerId: string | null;
  dragging: boolean;
}

const props = defineProps<{
  loading: boolean;
  initialized: boolean;
  loadError: string | null;
  providers: Provider[];
  livenessProviders: Provider[];
  regularProviders: Provider[];
  cliRuntime: CliRuntimeSnapshot;
  agentRuntimeSnapshot: AgentRuntimeSnapshot;
  runtimeLoading: boolean;
  activatingRuntimeId: string | null;
  announcementsLoaded: boolean;
  announcementsLoading: boolean;
  announcementTotalCount: number;
  announcementUnreadCount: number;
  announcementErrorCount: number;
  backgroundTasks: BackgroundTask[];
  recentBackgroundTasks: BackgroundTask[];
  backgroundTaskCount: number;
  switchingCliConfig: { providerId: string; cliKind: AgentCliKind } | null;
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
  checkingInProviderIds: string[];
  probingCapabilitiesProviderId: string | null;
  providerDrag: ProviderDragState;
  dragOverProviderId: string | null;
  draggedProvider: Provider | null;
  dragStyle: CSSProperties;
  providerCardTone: (provider: Provider) => ProviderCardTone;
  cardStatusTooltip: (provider: Provider) => string;
  showLivenessTimeline: (provider: Provider) => boolean;
}>();

const searchQuery = ref("");
const agentWorkspace = useAgentWorkspaceStore();
const agentOpened = ref(agentWorkspace.view === "agents");
const agentRefreshing = ref(false);
const dashboard = ref<InstanceType<typeof AgentDashboard> | null>(null);
watch(() => agentWorkspace.view, (view) => { if (view === "agents") agentOpened.value = true; });
const currentSearch = computed(() => agentWorkspace.view === "agents" ? agentWorkspace.query : searchQuery.value);

function updateSearch(value: string) {
  if (agentWorkspace.view === "agents") agentWorkspace.query = value;
  else searchQuery.value = value;
}

function refreshCurrentView() {
  if (agentWorkspace.view === "agents") void dashboard.value?.refresh();
  else emit("refreshAll");
}

function openRuntime(kind: AgentCliKind) {
  if (agentWorkspace.view === "agents") agentWorkspace.openSessions("active", kind);
  else emit("openAgentCliInstances", kind);
}

function matchesSearch(provider: Provider) {
  return providerMatchesSearch(provider, searchQuery.value);
}

const filteredLivenessProviders = computed(() => props.livenessProviders.filter(matchesSearch));
const filteredRegularProviders = computed(() => props.regularProviders.filter(matchesSearch));
function clearSearch() {
  searchQuery.value = "";
}

const emit = defineEmits<{
  startDrag: [event: MouseEvent];
  add: [];
  importData: [];
  retryLoad: [];
  checkForUpdate: [];
  openGithub: [];
  refreshAll: [];
  checkInAll: [];
  settings: [];
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
  openAgentCliInstances: [kind: AgentCliKind];
  openSiteAnnouncements: [];
  clearBackgroundTasks: [];
  switchCliConfig: [provider: Provider, cliKind: AgentCliKind];
  refreshRuntime: [];
  activateRuntime: [instance: AgentRuntimeSession];
}>();
</script>

<template>
  <AppTopbar
    :workspace-view="agentWorkspace.view"
    :refresh-in-progress="agentWorkspace.view === 'agents' ? agentRefreshing : refreshInProgress"
    :global-check-in-in-progress="globalCheckInInProgress"
    :search-query="currentSearch"
    :search-placeholder="agentWorkspace.view === 'agents' ? agentWorkspace.searchPlaceholder : undefined"
    :app-version="appVersion"
    :checking-for-update="checkingForUpdate"
    :cli-runtime="cliRuntime"
    :agent-runtime-snapshot="agentRuntimeSnapshot"
    :announcements-loaded="announcementsLoaded"
    :announcements-loading="announcementsLoading"
    :announcement-total-count="announcementTotalCount"
    :announcement-unread-count="announcementUnreadCount"
    :announcement-error-count="announcementErrorCount"
    :background-tasks="backgroundTasks"
    :recent-background-tasks="recentBackgroundTasks"
    :background-task-count="backgroundTaskCount"
    @start-drag="emit('startDrag', $event)"
    @set-search-query="updateSearch"
    @set-workspace-view="agentWorkspace.setView"
    @add="emit('add')"
    @import-data="emit('importData')"
    @check-for-update="emit('checkForUpdate')"
    @open-github="emit('openGithub')"
    @refresh="refreshCurrentView"
    @check-in="emit('checkInAll')"
    @open-cli="openRuntime"
    @open-announcements="emit('openSiteAnnouncements')"
    @clear-background-tasks="emit('clearBackgroundTasks')"
    @settings="emit('settings')"
  />

  <ProviderBoard
    v-show="agentWorkspace.view === 'providers'"
    :loading="loading"
    :initialized="initialized"
    :load-error="loadError"
    :providers="providers"
    :search-query="searchQuery"
    :liveness-providers="filteredLivenessProviders"
    :regular-providers="filteredRegularProviders"
    :cli-runtime="cliRuntime"
    :agent-runtime-snapshot="agentRuntimeSnapshot"
    :switching-cli-config="switchingCliConfig"
    :checking-in-provider-ids="checkingInProviderIds"
    :probing-capabilities-provider-id="probingCapabilitiesProviderId"
    :provider-drag="providerDrag"
    :drag-over-provider-id="dragOverProviderId"
    :dragged-provider="draggedProvider"
    :drag-style="dragStyle"
    :provider-card-tone="providerCardTone"
    :card-status-tooltip="cardStatusTooltip"
    :show-liveness-timeline="showLivenessTimeline"
    @add="emit('add')"
    @import-data="emit('importData')"
    @retry-load="emit('retryLoad')"
    @card-click="emit('cardClick', $event)"
    @card-pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
    @toggle="emit('toggle', $event)"
    @refresh="emit('refresh', $event)"
    @probe-capabilities="emit('probeCapabilities', $event)"
    @launch-temporary-cli="(provider, cliKind) => emit('launchTemporaryCli', provider, cliKind)"
    @edit="emit('edit', $event)"
    @check-in="emit('checkIn', $event)"
    @open-api-key-manager="emit('openApiKeyManager', $event)"
    @select-api-key="(provider, option) => emit('selectApiKey', provider, option)"
    @open-available-models="emit('openAvailableModels', $event)"
    @open-usage="emit('openUsage', $event)"
    @open-request-logs="emit('openRequestLogs', $event)"
    @open-password-change="emit('openPasswordChange', $event)"
    @open-liveness-details="emit('openLivenessDetails', $event)"
    @open-check-in-records="emit('openCheckInRecords', $event)"
    @add-cc-switch-config="(provider, target) => emit('addCcSwitchConfig', provider, target)"
    @copy-url="emit('copyUrl', $event)"
    @copy-invite="emit('copyInvite', $event)"
    @copy-secret="(provider, field) => emit('copySecret', provider, field)"
    @remove="emit('remove', $event)"
    @open-cli-instances="(provider, cliKind) => emit('openCliInstances', provider, cliKind)"
    @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
    @clear-search="clearSearch"
  />
  <AgentDashboard
    v-if="agentOpened"
    v-show="agentWorkspace.view === 'agents'"
    ref="dashboard"
    :active="agentWorkspace.view === 'agents'"
    :providers="providers"
    :cli-runtime="cliRuntime"
    :runtime-snapshot="agentRuntimeSnapshot"
    :runtime-loading="runtimeLoading"
    :activating-id="activatingRuntimeId"
    @refreshing="agentRefreshing = $event"
    @refresh-runtime="emit('refreshRuntime')"
    @activate-runtime="emit('activateRuntime', $event)"
    @launch="(provider, kind) => emit('launchTemporaryCli', provider, kind)"
  />
</template>
