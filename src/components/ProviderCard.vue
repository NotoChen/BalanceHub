<script setup lang="ts">
import { computed, type CSSProperties } from "vue";
import type { LivenessCliKind, Provider } from "../stores/providers";
import type { ProviderCardTone } from "../utils/provider-display";
import type { CcSwitchAppTarget } from "../utils/ccswitch-deeplink";
import ProviderCardHeader from "./provider-card/ProviderCardHeader.vue";
import ProviderCardBody from "./provider-card/ProviderCardBody.vue";
import ProviderCardActions from "./provider-card/ProviderCardActions.vue";

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
    codexDefault?: boolean;
    claudeDefault?: boolean;
    codexActiveCliCount?: number;
    claudeActiveCliCount?: number;
    switchingCliKind?: LivenessCliKind | null;
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
    codexDefault: false,
    claudeDefault: false,
    codexActiveCliCount: 0,
    claudeActiveCliCount: 0,
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
  openCliInstances: [provider: Provider, cliKind: LivenessCliKind];
  switchCliConfig: [provider: Provider, cliKind: LivenessCliKind];
  probeCapabilities: [provider: Provider];
  openApiKeyManager: [provider: Provider];
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
}>();

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const isGenericApi = computed(() => props.provider.identity.protocol === "api");

const actionListeners = {
  switchCliConfig: (provider: Provider, cliKind: LivenessCliKind) =>
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

function forwardOpenCliInstances(provider: Provider, cliKind: LivenessCliKind) {
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
      },
    ]"
    :role="interactive ? 'group' : undefined"
    :aria-disabled="interactive ? !provider.runtime.enabled : undefined"
    :aria-hidden="ariaHidden || undefined"
    :aria-label="interactive ? `${provider.identity.name} 中转站卡片` : undefined"
    :tabindex="interactive ? 0 : undefined"
    :title="title"
    :style="dragStyle"
    @click="handleClick"
    @dragstart.prevent
    @pointerdown="handlePointerDown"
    @keydown.enter="handleEnter"
  >
    <ProviderCardHeader
      :provider="provider"
      :tone="tone"
      :title="title"
      :interactive="interactive"
      :codex-default="codexDefault"
      :claude-default="claudeDefault"
      :codex-active-cli-count="codexActiveCliCount"
      :claude-active-cli-count="claudeActiveCliCount"
      @open-cli-instances="forwardOpenCliInstances"
    />

    <div class="provider-card-content">
      <ProviderCardBody
        :provider="provider"
        :show-liveness-timeline="showLivenessTimeline"
      />
      <ProviderCardActions
        :provider="provider"
        :interactive="interactive"
        :codex-default="codexDefault"
        :claude-default="claudeDefault"
        :switching-cli-kind="switchingCliKind"
        :cli-config-switching="cliConfigSwitching"
        :probing-capabilities="probingCapabilities"
        :checking-in="checkingIn"
        v-on="actionListeners"
      />
    </div>
  </article>
</template>
