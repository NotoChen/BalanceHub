<script setup lang="ts">
import { type CSSProperties } from "vue";
import AppTopbar from "./AppTopbar.vue";
import ProviderBoard from "./ProviderBoard.vue";
import type { CliRuntimeSnapshot, LivenessCliKind, Provider } from "../stores/providers";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import type { ProviderCardTone } from "../utils/provider-display";

interface ProviderContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  provider: Provider | null;
}

interface ProviderDragState {
  providerId: string | null;
  dragging: boolean;
}

defineProps<{
  loading: boolean;
  initialized: boolean;
  loadError: string | null;
  providers: Provider[];
  livenessProviders: Provider[];
  regularProviders: Provider[];
  cliRuntime: CliRuntimeSnapshot;
  refreshInProgress: boolean;
  globalCheckInInProgress: boolean;
  providerContextMenu: ProviderContextMenuState;
  checkingInProviderIds: string[];
  probingCapabilitiesProviderId: string | null;
  testingLivenessProviderId: string | null;
  providerDrag: ProviderDragState;
  dragOverProviderId: string | null;
  draggedProvider: Provider | null;
  dragStyle: CSSProperties;
  providerCardTone: (provider: Provider) => ProviderCardTone;
  cardStatusTooltip: (provider: Provider) => string;
  showLivenessTimeline: (provider: Provider) => boolean;
}>();

const emit = defineEmits<{
  startDrag: [event: MouseEvent];
  add: [];
  importData: [];
  refreshAll: [];
  checkInAll: [];
  settings: [];
  cardClick: [provider: Provider];
  cardContextmenu: [provider: Provider, event: MouseEvent];
  cardPointerdown: [provider: Provider, event: PointerEvent];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  probeCapabilities: [provider: Provider];
  testLiveness: [provider: Provider];
  launchTemporaryCli: [provider: Provider, cliKind: LivenessCliKind];
  edit: [provider: Provider];
  checkIn: [provider: Provider];
  openApiKeyManager: [provider: Provider];
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
  openCliInstances: [provider: Provider];
}>();
</script>

<template>
  <AppTopbar
    :refresh-in-progress="refreshInProgress"
    :global-check-in-in-progress="globalCheckInInProgress"
    @start-drag="emit('startDrag', $event)"
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
    :liveness-providers="livenessProviders"
    :regular-providers="regularProviders"
    :cli-runtime="cliRuntime"
    :provider-context-menu="providerContextMenu"
    :checking-in-provider-ids="checkingInProviderIds"
    :probing-capabilities-provider-id="probingCapabilitiesProviderId"
    :testing-liveness-provider-id="testingLivenessProviderId"
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
    @card-contextmenu="(provider, event) => emit('cardContextmenu', provider, event)"
    @card-pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
    @toggle="emit('toggle', $event)"
    @refresh="emit('refresh', $event)"
    @probe-capabilities="emit('probeCapabilities', $event)"
    @test-liveness="emit('testLiveness', $event)"
    @launch-temporary-cli="(provider, cliKind) => emit('launchTemporaryCli', provider, cliKind)"
    @edit="emit('edit', $event)"
    @check-in="emit('checkIn', $event)"
    @open-api-key-manager="emit('openApiKeyManager', $event)"
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
    @open-cli-instances="emit('openCliInstances', $event)"
  />
</template>
