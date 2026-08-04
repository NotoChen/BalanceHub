<script setup lang="ts">
import { computed, type CSSProperties } from "vue";
import ProviderCard from "./ProviderCard.vue";
import type { CliRuntimeSnapshot, LivenessCliKind, Provider } from "../stores/providers";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import type { ProviderCardTone } from "../utils/provider-display";

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

const emit = defineEmits<{
  add: [];
  importData: [];
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
  resetFilters: [];
}>();

const filteredLivenessProviders = computed(() => props.livenessProviders);
const filteredRegularProviders = computed(() => props.regularProviders);
const accountProviders = computed(() =>
  filteredRegularProviders.value.filter((provider) => provider.auth.mode !== "apiKey"),
);
const apiKeyProviders = computed(() =>
  filteredRegularProviders.value.filter((provider) => provider.auth.mode === "apiKey"),
);
const visibleProviderCount = computed(
  () => filteredLivenessProviders.value.length + filteredRegularProviders.value.length,
);
function providerIsCliDefault(provider: Provider, cliKind: LivenessCliKind) {
  return props.cliRuntime[cliKind].providerId === provider.identity.id;
}

function providerActiveCliCount(provider: Provider, cliKind: LivenessCliKind) {
  return props.cliRuntime.instances.filter(
    (instance) =>
      instance.providerId === provider.identity.id &&
      instance.cliKind === cliKind &&
      instance.status !== "exited",
  ).length;
}

function providerSwitchingCliKind(provider: Provider) {
  return props.switchingCliConfig?.providerId === provider.identity.id
    ? props.switchingCliConfig.cliKind
    : null;
}
</script>

<template>
  <section class="content provider-board">
    <a-spin v-if="loading && !initialized" tip="正在加载本地配置..." />

    <a-alert v-if="loadError" type="error" show-icon class="provider-load-error">
      <template #title>本地配置未加载</template>
      <div class="provider-load-error-content">
        <span>{{ loadError }}</span>
        <a-button size="small" @click="emit('importData')">导入配置</a-button>
      </div>
    </a-alert>

    <section v-if="!loadError && filteredLivenessProviders.length > 0" class="provider-board-section">
      <div class="provider-board-section-header">
        <h2>自动测活</h2>
        <span>{{ filteredLivenessProviders.length }}</span>
      </div>
      <TransitionGroup name="provider-grid" tag="div" class="overview-provider-grid">
        <ProviderCard
          v-for="provider in filteredLivenessProviders"
          :key="provider.identity.id"
          :provider="provider"
          :tone="providerCardTone(provider)"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="true"
          :codex-default="providerIsCliDefault(provider, 'codex')"
          :claude-default="providerIsCliDefault(provider, 'claudeCode')"
          :codex-active-cli-count="providerActiveCliCount(provider, 'codex')"
          :claude-active-cli-count="providerActiveCliCount(provider, 'claudeCode')"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :passing-challenge="challengingProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="(provider, cliKind) => emit('openCliInstances', provider, cliKind)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
          @open-available-models="emit('openAvailableModels', $event)"
          @open-usage="emit('openUsage', $event)"
          @open-request-logs="emit('openRequestLogs', $event)"
          @open-password-change="emit('openPasswordChange', $event)"
          @pass-challenge="emit('passChallenge', $event)"
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

    <section v-if="!loadError && accountProviders.length > 0" class="provider-board-section">
      <div class="provider-board-section-header">
        <h2>账户认证</h2>
        <span>{{ accountProviders.length }}</span>
      </div>
      <TransitionGroup name="provider-grid" tag="div" class="overview-provider-grid">
        <ProviderCard
          v-for="provider in accountProviders"
          :key="provider.identity.id"
          :provider="provider"
          :tone="providerCardTone(provider)"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="false"
          :codex-default="providerIsCliDefault(provider, 'codex')"
          :claude-default="providerIsCliDefault(provider, 'claudeCode')"
          :codex-active-cli-count="providerActiveCliCount(provider, 'codex')"
          :claude-active-cli-count="providerActiveCliCount(provider, 'claudeCode')"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :passing-challenge="challengingProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="(provider, cliKind) => emit('openCliInstances', provider, cliKind)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
          @open-available-models="emit('openAvailableModels', $event)"
          @open-usage="emit('openUsage', $event)"
          @open-request-logs="emit('openRequestLogs', $event)"
          @open-password-change="emit('openPasswordChange', $event)"
          @pass-challenge="emit('passChallenge', $event)"
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

    <section v-if="!loadError && apiKeyProviders.length > 0" class="provider-board-section">
      <div class="provider-board-section-header">
        <h2>API Key</h2>
        <span>{{ apiKeyProviders.length }}</span>
      </div>
      <TransitionGroup name="provider-grid" tag="div" class="overview-provider-grid">
        <ProviderCard
          v-for="provider in apiKeyProviders"
          :key="provider.identity.id"
          :provider="provider"
          :tone="providerCardTone(provider)"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="false"
          :codex-default="providerIsCliDefault(provider, 'codex')"
          :claude-default="providerIsCliDefault(provider, 'claudeCode')"
          :codex-active-cli-count="providerActiveCliCount(provider, 'codex')"
          :claude-active-cli-count="providerActiveCliCount(provider, 'claudeCode')"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :passing-challenge="challengingProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="(provider, cliKind) => emit('openCliInstances', provider, cliKind)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
          @open-available-models="emit('openAvailableModels', $event)"
          @open-usage="emit('openUsage', $event)"
          @open-request-logs="emit('openRequestLogs', $event)"
          @open-password-change="emit('openPasswordChange', $event)"
          @pass-challenge="emit('passChallenge', $event)"
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

    <div v-if="!loadError && providers.length === 0 && !loading" class="empty-state">
      <h3>还没有中转站</h3>
      <p>添加中转站地址后会尝试读取站点名称，再配置认证方式。</p>
      <a-button type="primary" @click="emit('add')">添加中转站</a-button>
    </div>

    <div
      v-else-if="!loadError && providers.length > 0 && visibleProviderCount === 0"
      class="empty-state provider-board-filter-empty"
    >
      <h3>没有匹配的中转站</h3>
      <p>当前认证方式或状态筛选没有结果。</p>
      <a-button @click="emit('resetFilters')">重置筛选</a-button>
    </div>

    <ProviderCard
      v-if="draggedProvider"
      :provider="draggedProvider"
      :tone="providerCardTone(draggedProvider)"
      :dragging="true"
      :interactive="false"
      :drag-style="dragStyle"
      :show-liveness-timeline="showLivenessTimeline(draggedProvider)"
      :codex-default="providerIsCliDefault(draggedProvider, 'codex')"
      :claude-default="providerIsCliDefault(draggedProvider, 'claudeCode')"
      :codex-active-cli-count="providerActiveCliCount(draggedProvider, 'codex')"
      :claude-active-cli-count="providerActiveCliCount(draggedProvider, 'claudeCode')"
      aria-hidden
    />
  </section>
</template>
