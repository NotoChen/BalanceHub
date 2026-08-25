<script setup lang="ts">
import { computed, type CSSProperties } from "vue";
import ProviderCard from "./ProviderCard.vue";
import type {
  CliRuntimeSnapshot,
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
}>();
const cliStore = useCliRuntimeStore();

const emit = defineEmits<{
  add: [];
  importData: [];
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
  return props.cliRuntime.instances.reduce<Partial<Record<AgentCliKind, number>>>(
    (counts, instance) => {
      if (instance.providerId === provider.identity.id && instance.status !== "exited") {
        counts[instance.cliKind] = (counts[instance.cliKind] || 0) + 1;
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

    <div v-if="!loadError && providers.length === 0 && !loading" class="empty-state">
      <h3>还没有中转站</h3>
      <p>添加中转站地址后会尝试读取站点名称，再配置认证方式。</p>
      <a-button type="primary" @click="emit('add')">添加中转站</a-button>
    </div>

    <div
      v-else-if="!loadError && providers.length > 0 && visibleProviderCount === 0"
      class="empty-state provider-board-search-empty"
    >
      <h3>没有匹配的中转站</h3>
      <p>当前搜索条件没有结果。</p>
      <a-button @click="emit('clearSearch')">清除搜索</a-button>
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
