<script setup lang="ts">
import { computed, ref, type CSSProperties } from "vue";
import type { AgentCliKind, Provider, ProviderApiKeyOption } from "../stores/providers";
import type { ProviderCardTone } from "../utils/provider-display";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import ProviderCardHeader from "./provider-card/ProviderCardHeader.vue";
import ProviderCardBody from "./provider-card/ProviderCardBody.vue";
import ProviderCardActions from "./provider-card/ProviderCardActions.vue";
import ProviderCardCliOrbits from "./provider-card/ProviderCardCliOrbits.vue";
import type { ProviderCardCliOrbitSpec } from "../utils/provider-card-cli-orbit";
import { providerCardTitle } from "../utils/provider-display";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    tone: ProviderCardTone;
    title?: string;
    interactive?: boolean;
    placeholder?: boolean;
    dragOver?: boolean;
    dragging?: boolean;
    dragStyle?: CSSProperties;
    showLivenessTimeline?: boolean;
    cliOrbits?: readonly ProviderCardCliOrbitSpec[];
    activeCliCounts?: Partial<Record<AgentCliKind, number>>;
    switchingCliKind?: AgentCliKind | null;
    cliConfigSwitching?: boolean;
    probingCapabilities?: boolean;
    checkingIn?: boolean;
    ariaHidden?: boolean;
  }>(),
  {
    title: "",
    interactive: true,
    placeholder: false,
    dragOver: false,
    dragging: false,
    dragStyle: undefined,
    showLivenessTimeline: false,
    cliOrbits: () => [],
    activeCliCounts: () => ({}),
    switchingCliKind: null,
    cliConfigSwitching: false,
    probingCapabilities: false,
    checkingIn: false,
    ariaHidden: false,
  },
);

const emit = defineEmits<{
  click: [provider: Provider, event: MouseEvent];
  pointerdown: [provider: Provider, event: PointerEvent];
  enter: [provider: Provider, event: KeyboardEvent];
  openCliInstances: [provider: Provider, cliKind: AgentCliKind];
  switchCliConfig: [provider: Provider, cliKind: AgentCliKind];
  probeCapabilities: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  selectApiKey: [provider: Provider, option: ProviderApiKeyOption];
  openAvailableModels: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addCcSwitchConfig: [provider: Provider, target: CcSwitchAppTarget];
  launchTemporaryCli: [provider: Provider];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  edit: [provider: Provider];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  checkIn: [provider: Provider];
  remove: [provider: Provider];
  interaction: [active: boolean];
}>();

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const isGenericApi = computed(() => props.provider.identity.protocol === "api");
const interactionActive = ref(false);

const actionListeners = {
  switchCliConfig: (provider: Provider, cliKind: AgentCliKind) =>
    emit("switchCliConfig", provider, cliKind),
  probeCapabilities: (provider: Provider) => emit("probeCapabilities", provider),
  openApiKeyManager: (provider: Provider) => emit("openApiKeyManager", provider),
  openAvailableModels: (provider: Provider) => emit("openAvailableModels", provider),
  openUsage: (provider: Provider) => emit("openUsage", provider),
  openRequestLogs: (provider: Provider) => emit("openRequestLogs", provider),
  openPasswordChange: (provider: Provider) => emit("openPasswordChange", provider),
  openLivenessDetails: (provider: Provider) => emit("openLivenessDetails", provider),
  openCheckInRecords: (provider: Provider) => emit("openCheckInRecords", provider),
  addCcSwitchConfig: (provider: Provider, target: CcSwitchAppTarget) =>
    emit("addCcSwitchConfig", provider, target),
  launchTemporaryCli: (provider: Provider) => emit("launchTemporaryCli", provider),
  copyUrl: (provider: Provider) => emit("copyUrl", provider),
  copyInvite: (provider: Provider) => emit("copyInvite", provider),
  copySecret: (
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ) => emit("copySecret", provider, field),
  edit: (provider: Provider) => emit("edit", provider),
  toggle: (provider: Provider) => emit("toggle", provider),
  refresh: (provider: Provider) => emit("refresh", provider),
  checkIn: (provider: Provider) => emit("checkIn", provider),
  remove: (provider: Provider) => emit("remove", provider),
  interaction: (active: boolean) => {
    interactionActive.value = active;
    emit("interaction", active);
  },
};

function handleClick(event: MouseEvent) {
  if (props.interactive) {
    emit("click", props.provider, event);
  }
}

function handlePointerDown(event: PointerEvent) {
  if (props.interactive) {
    emit("pointerdown", props.provider, event);
  }
}

function handleEnter(event: KeyboardEvent) {
  if (props.interactive) {
    emit("enter", props.provider, event);
  }
}

function forwardOpenCliInstances(provider: Provider, cliKind: AgentCliKind) {
  emit("openCliInstances", provider, cliKind);
}
</script>

<template>
  <article
    :data-provider-id="provider.identity.id"
    class="provider-card"
    :class="[
      `provider-card-${tone}`,
      {
        'provider-card-disabled': tone === 'disabled',
        'provider-card-placeholder': placeholder,
        'provider-card-drag-over': dragOver,
        'provider-card-dragging': dragging,
        'provider-card-api-key': isApiKeyAuth,
        'provider-card-generic-api': isGenericApi,
        'provider-card-standard': !showLivenessTimeline,
        'provider-card-has-cli-orbits': cliOrbits.length > 0,
        'provider-card-interacting': interactionActive,
      },
    ]"
    :role="interactive ? 'group' : undefined"
    :aria-disabled="interactive ? !provider.runtime.enabled : undefined"
    :aria-hidden="ariaHidden || undefined"
    :aria-label="interactive ? `${providerCardTitle(provider)} 中转站卡片` : undefined"
    :tabindex="interactive ? 0 : undefined"
    :title="title"
    :style="dragStyle"
    @click="handleClick"
    @dragstart.prevent
    @pointerdown="handlePointerDown"
    @keydown.enter="handleEnter"
  >
    <ProviderCardCliOrbits :orbits="cliOrbits" />
    <ProviderCardHeader
      :provider="provider"
      :tone="tone"
      :title="title"
      :interactive="interactive"
      :active-cli-counts="activeCliCounts"
      @open-cli-instances="forwardOpenCliInstances"
      @copy-api-key="emit('copySecret', $event, 'apiKey')"
      @manage-api-keys="emit('openApiKeyManager', $event)"
      @select-api-key="(provider, option) => emit('selectApiKey', provider, option)"
    />

    <div class="provider-card-content">
      <ProviderCardBody
        :provider="provider"
        :show-liveness-timeline="showLivenessTimeline"
      />
      <ProviderCardActions
        :provider="provider"
        :interactive="interactive"
        :switching-cli-kind="switchingCliKind"
        :cli-config-switching="cliConfigSwitching"
        :probing-capabilities="probingCapabilities"
        :checking-in="checkingIn"
        v-on="actionListeners"
      />
    </div>
  </article>
</template>
