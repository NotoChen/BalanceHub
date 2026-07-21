<script setup lang="ts">
import { computed, ref, type CSSProperties } from "vue";
import { IconRefresh } from "@arco-design/web-vue/es/icon";
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
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addCcSwitchConfig: [provider: Provider, target: CcSwitchAppTarget];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  remove: [provider: Provider];
  openCliInstances: [provider: Provider];
  switchCliConfig: [provider: Provider, cliKind: LivenessCliKind];
}>();

type AuthFilter = "all" | "account" | "apiKey";
type StatusFilter = "all" | "warning" | "error";

const authFilter = ref<AuthFilter>("all");
const statusFilter = ref<StatusFilter>("all");
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
const accountProviders = computed(() =>
  filteredRegularProviders.value.filter((provider) => provider.auth.mode !== "apiKey"),
);
const apiKeyProviders = computed(() =>
  filteredRegularProviders.value.filter((provider) => provider.auth.mode === "apiKey"),
);
const visibleProviderCount = computed(
  () => filteredLivenessProviders.value.length + filteredRegularProviders.value.length,
);
const hasActiveFilters = computed(
  () => authFilter.value !== "all" || statusFilter.value !== "all",
);

function resetFilters() {
  authFilter.value = "all";
  statusFilter.value = "all";
}

function toggleStatusFilter(value: Exclude<StatusFilter, "all">) {
  statusFilter.value = statusFilter.value === value ? "all" : value;
}

function providerIsCliDefault(provider: Provider, cliKind: LivenessCliKind) {
  return props.cliRuntime[cliKind].providerId === provider.identity.id;
}

function providerActiveCliCount(provider: Provider) {
  return props.cliRuntime.instances.filter(
    (instance) => instance.providerId === provider.identity.id && instance.status !== "exited",
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

    <div v-if="!loadError && providers.length > 0" class="provider-board-toolbar">
      <div class="provider-board-filters" aria-label="中转站筛选">
        <span class="provider-board-filter-label">认证</span>
        <div class="provider-board-filter-segment" role="group" aria-label="按认证方式筛选">
          <button
            type="button"
            :class="{ active: authFilter === 'all' }"
            :aria-pressed="authFilter === 'all'"
            @click="authFilter = 'all'"
          >
            全部
          </button>
          <button
            type="button"
            :class="{ active: authFilter === 'account' }"
            :aria-pressed="authFilter === 'account'"
            @click="authFilter = 'account'"
          >
            账户认证
          </button>
          <button
            type="button"
            :class="{ active: authFilter === 'apiKey' }"
            :aria-pressed="authFilter === 'apiKey'"
            @click="authFilter = 'apiKey'"
          >
            API Key
          </button>
        </div>
        <span class="provider-board-filter-label provider-board-status-label">状态</span>
        <div class="provider-board-filter-segment" role="group" aria-label="按状态筛选">
          <button
            type="button"
            :class="{ active: statusFilter === 'warning' }"
            :aria-pressed="statusFilter === 'warning'"
            @click="toggleStatusFilter('warning')"
          >
            未签到
          </button>
          <button
            type="button"
            :class="{ active: statusFilter === 'error' }"
            :aria-pressed="statusFilter === 'error'"
            @click="toggleStatusFilter('error')"
          >
            异常
          </button>
        </div>
        <a-button
          v-if="hasActiveFilters"
          type="text"
          size="small"
          class="provider-board-filter-reset"
          title="重置筛选"
          aria-label="重置筛选"
          @click="resetFilters"
        >
          <icon-refresh />
        </a-button>
      </div>
      <div class="provider-board-filter-summary" aria-live="polite">
        <strong>{{ visibleProviderCount }}</strong>
        <span>/ {{ providers.length }}</span>
      </div>
    </div>

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
          :active-cli-count="providerActiveCliCount(provider)"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="emit('openCliInstances', $event)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
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
          :codex-default="providerIsCliDefault(provider, 'codex')"
          :claude-default="providerIsCliDefault(provider, 'claudeCode')"
          :active-cli-count="providerActiveCliCount(provider)"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="emit('openCliInstances', $event)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
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
          :codex-default="providerIsCliDefault(provider, 'codex')"
          :claude-default="providerIsCliDefault(provider, 'claudeCode')"
          :active-cli-count="providerActiveCliCount(provider)"
          :switching-cli-kind="providerSwitchingCliKind(provider)"
          :cli-config-switching="Boolean(switchingCliConfig)"
          :probing-capabilities="probingCapabilitiesProviderId === provider.identity.id"
          :checking-in="checkingInProviderIds.includes(provider.identity.id)"
          @click="emit('cardClick', $event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
          @open-cli-instances="emit('openCliInstances', $event)"
          @switch-cli-config="(provider, cliKind) => emit('switchCliConfig', provider, cliKind)"
          @probe-capabilities="emit('probeCapabilities', $event)"
          @open-api-key-manager="emit('openApiKeyManager', $event)"
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
      class="empty-state provider-board-filter-empty"
    >
      <h3>没有匹配的中转站</h3>
      <p>当前认证方式或状态筛选没有结果。</p>
      <a-button @click="resetFilters">重置筛选</a-button>
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
      :active-cli-count="providerActiveCliCount(draggedProvider)"
      aria-hidden
    />
  </section>
</template>
