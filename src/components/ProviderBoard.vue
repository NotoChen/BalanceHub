<script setup lang="ts">
import { type CSSProperties } from "vue";
import ProviderCard from "./ProviderCard.vue";
import ProviderContextMenu from "./ProviderContextMenu.vue";
import type { Provider } from "../stores/providers";
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
  providerContextMenu: ProviderContextMenuState;
  checkingInProviderId: string | null;
  syncingCapabilitiesProviderId: string | null;
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
  add: [];
  importData: [];
  cardClick: [provider: Provider];
  cardContextmenu: [provider: Provider, event: MouseEvent];
  cardPointerdown: [provider: Provider, event: PointerEvent];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  syncCapabilities: [provider: Provider];
  testLiveness: [provider: Provider];
  edit: [provider: Provider];
  checkIn: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  remove: [provider: Provider];
}>();
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

    <section v-if="!loadError && livenessProviders.length > 0" class="provider-board-section">
      <TransitionGroup name="provider-grid" tag="div" class="overview-provider-grid">
        <ProviderCard
          v-for="provider in livenessProviders"
          :key="provider.identity.id"
          :provider="provider"
          :tone="providerCardTone(provider)"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="true"
          @click="emit('cardClick', $event)"
          @contextmenu="(provider, event) => emit('cardContextmenu', provider, event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
        />
      </TransitionGroup>
    </section>

    <section v-if="!loadError && (regularProviders.length > 0 || (providers.length === 0 && !loading))" class="provider-board-section">
      <div v-if="regularProviders.length > 0 && livenessProviders.length > 0" class="provider-board-divider" />
      <TransitionGroup name="provider-grid" tag="div" class="overview-provider-grid">
        <ProviderCard
          v-for="provider in regularProviders"
          :key="provider.identity.id"
          :provider="provider"
          :tone="providerCardTone(provider)"
          :placeholder="providerDrag.providerId === provider.identity.id && providerDrag.dragging"
          :drag-over="dragOverProviderId === provider.identity.id"
          :title="cardStatusTooltip(provider)"
          :show-liveness-timeline="false"
          @click="emit('cardClick', $event)"
          @contextmenu="(provider, event) => emit('cardContextmenu', provider, event)"
          @pointerdown="(provider, event) => emit('cardPointerdown', provider, event)"
          @enter="emit('cardClick', $event)"
        />
      <div v-if="providers.length === 0 && !loading" key="empty-state" class="empty-state">
        <h3>还没有中转站</h3>
        <p>添加中转站地址后会尝试读取站点名称，再配置认证方式。</p>
        <a-button type="primary" @click="emit('add')">添加中转站</a-button>
      </div>
      </TransitionGroup>
    </section>

    <ProviderContextMenu
      v-if="providerContextMenu.visible && providerContextMenu.provider"
      :provider="providerContextMenu.provider"
      :x="providerContextMenu.x"
      :y="providerContextMenu.y"
      :checking-in="checkingInProviderId === providerContextMenu.provider.identity.id"
      :syncing-capabilities="syncingCapabilitiesProviderId === providerContextMenu.provider.identity.id"
      :testing-liveness="testingLivenessProviderId === providerContextMenu.provider.identity.id"
      @toggle="emit('toggle', $event)"
      @refresh="emit('refresh', $event)"
      @sync-capabilities="emit('syncCapabilities', $event)"
      @test-liveness="emit('testLiveness', $event)"
      @edit="emit('edit', $event)"
      @check-in="emit('checkIn', $event)"
      @open-api-key-manager="emit('openApiKeyManager', $event)"
      @open-usage="emit('openUsage', $event)"
      @open-request-logs="emit('openRequestLogs', $event)"
      @open-password-change="emit('openPasswordChange', $event)"
      @open-liveness-details="emit('openLivenessDetails', $event)"
      @open-check-in-records="emit('openCheckInRecords', $event)"
      @copy-url="emit('copyUrl', $event)"
      @copy-invite="emit('copyInvite', $event)"
      @copy-secret="(provider, field) => emit('copySecret', provider, field)"
      @remove="emit('remove', $event)"
    />

    <ProviderCard
      v-if="draggedProvider"
      :provider="draggedProvider"
      :tone="providerCardTone(draggedProvider)"
      :dragging="true"
      :interactive="false"
      :drag-style="dragStyle"
      :show-liveness-timeline="showLivenessTimeline(draggedProvider)"
      aria-hidden
    />
  </section>
</template>
