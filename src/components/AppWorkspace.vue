<script setup lang="ts">
import { computed, ref, type CSSProperties } from "vue";
import AppTopbar from "./AppTopbar.vue";
import ProviderBoard from "./ProviderBoard.vue";
import type { CliRuntimeSnapshot, LivenessCliKind, Provider } from "../stores/providers";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import type { ProviderCardTone } from "../utils/provider-display";
import type { ProviderAuthFilter, ProviderStatusFilter } from "../utils/provider-filters";

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
  switchingCliConfig: { providerId: string; cliKind: LivenessCliKind } | null;
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  checkingInProviderIds: string[];
  probingCapabilitiesProviderId: string | null;
  challengingProviderId: string | null;
  providerDrag: ProviderDragState;
  dragOverProviderId: string | null;
  draggedProvider: Provider | null;
  dragStyle: CSSProperties;
  providerCardTone: (provider: Provider) => ProviderCardTone;
  cardStatusTooltip: (provider: Provider) => string;
  showLivenessTimeline: (provider: Provider) => boolean;
}>();

const authFilter = ref<ProviderAuthFilter>("all");
const statusFilter = ref<ProviderStatusFilter>("all");

function matchesFilters(provider: Provider) {
  const authMatches =
    authFilter.value === "all" ||
    (authFilter.value === "apiKey" && provider.auth.mode === "apiKey") ||
    (authFilter.value === "account" && provider.auth.mode !== "apiKey");
  const statusMatches =
    statusFilter.value === "all" || props.providerCardTone(provider) === statusFilter.value;
  return authMatches && statusMatches;
}

const filteredLivenessProviders = computed(() => props.livenessProviders.filter(matchesFilters));
const filteredRegularProviders = computed(() => props.regularProviders.filter(matchesFilters));
const hasActiveFilters = computed(
  () => authFilter.value !== "all" || statusFilter.value !== "all",
);

function setAuthFilter(value: ProviderAuthFilter) {
  authFilter.value = value;
}

function toggleStatusFilter(value: Exclude<ProviderStatusFilter, "all">) {
  statusFilter.value = statusFilter.value === value ? "all" : value;
}

function resetFilters() {
  authFilter.value = "all";
  statusFilter.value = "all";
}

const emit = defineEmits<{
  startDrag: [event: MouseEvent];
  add: [];
  importData: [];
  refreshAll: [];
  checkInAll: [];
  settings: [];
  cardClick: [provider: Provider];
  cardPointerdown: [provider: Provider, event: PointerEvent];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  probeCapabilities: [provider: Provider];
  launchTemporaryCli: [provider: Provider, cliKind?: LivenessCliKind];
  edit: [provider: Provider];
  checkIn: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  openAvailableModels: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  passChallenge: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addCcSwitchConfig: [provider: Provider, target: CcSwitchAppTarget];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  remove: [provider: Provider];
  openCliInstances: [provider: Provider, cliKind: LivenessCliKind];
  switchCliConfig: [provider: Provider, cliKind: LivenessCliKind];
}>();
</script>

<template>
  <AppTopbar
    :refresh-in-progress="refreshInProgress"
    :global-check-in-in-progress="globalCheckInInProgress"
    :auth-filter="authFilter"
    :status-filter="statusFilter"
    :has-active-filters="hasActiveFilters"
    @start-drag="emit('startDrag', $event)"
    @set-auth-filter="setAuthFilter"
    @toggle-status-filter="toggleStatusFilter"
    @reset-filters="resetFilters"
    @add="emit('add')"
    @refresh="emit('refreshAll')"
    @check-in="emit('checkInAll')"
    @settings="emit('settings')"
  />

  <ProviderBoard
    :loading="loading"
    :initialized="initialized"
    :load-error="loadError"
    :providers="providers"
    :liveness-providers="filteredLivenessProviders"
    :regular-providers="filteredRegularProviders"
    :cli-runtime="cliRuntime"
    :switching-cli-config="switchingCliConfig"
    :checking-in-provider-ids="checkingInProviderIds"
    :probing-capabilities-provider-id="probingCapabilitiesProviderId"
    :challenging-provider-id="challengingProviderId"
    :provider-drag="providerDrag"
    :drag-over-provider-id="dragOverProviderId"
    :dragged-provider="draggedProvider"
    :drag-style="dragStyle"
    :provider-card-tone="providerCardTone"
    :card-status-tooltip="cardStatusTooltip"
    :show-liveness-timeline="showLivenessTimeline"
    @add="emit('add')"
    @import-data="emit('importData')"
    @card-click="emit('cardClick', $event)"
    @card-pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
    @toggle="emit('toggle', $event)"
    @refresh="emit('refresh', $event)"
    @probe-capabilities="emit('probeCapabilities', $event)"
    @launch-temporary-cli="(provider, cliKind) => emit('launchTemporaryCli', provider, cliKind)"
    @edit="emit('edit', $event)"
    @check-in="emit('checkIn', $event)"
    @open-api-key-manager="emit('openApiKeyManager', $event)"
    @open-available-models="emit('openAvailableModels', $event)"
    @open-usage="emit('openUsage', $event)"
    @open-request-logs="emit('openRequestLogs', $event)"
    @open-password-change="emit('openPasswordChange', $event)"
    @pass-challenge="emit('passChallenge', $event)"
    @open-liveness-details="emit('openLivenessDetails', $event)"
    @open-check-in-records="emit('openCheckInRecords', $event)"
    @add-cc-switch-config="(provider, target) => emit('addCcSwitchConfig', provider, target)"
    @copy-url="emit('copyUrl', $event)"
    @copy-invite="emit('copyInvite', $event)"
    @copy-secret="(provider, field) => emit('copySecret', provider, field)"
    @remove="emit('remove', $event)"
    @open-cli-instances="(provider, cliKind) => emit('openCliInstances', provider, cliKind)"
    @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
    @reset-filters="resetFilters"
  />
</template>
