<script setup lang="ts">
import { computed, type CSSProperties } from "vue";
import { IconCode } from "@arco-design/web-vue/es/icon";
import ProviderLivenessTimeline from "./ProviderLivenessTimeline.vue";
import type { Provider } from "../stores/providers";
import {
  availablePercent,
  availablePercentLabel,
  formatProviderQuota,
  providerAvailableQuotaLabel,
  providerAuthModeDescription,
  providerAuthModeLabel,
  providerIdentityId,
  providerIdentityName,
  providerIdentitySecondaryUsername,
  providerQuotaScopeLabel,
  providerQuotaUnlimited,
  providerTotalQuotaLabel,
  type ProviderCardTone,
} from "../utils/provider-display";
import newApiLogo from "../assets/logos/new-api.png";

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
    activeCliCount?: number;
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
    activeCliCount: 0,
    ariaHidden: false,
  },
);

const emit = defineEmits<{
  click: [provider: Provider, event: MouseEvent];
  contextmenu: [provider: Provider, event: MouseEvent];
  pointerdown: [provider: Provider, event: PointerEvent];
  enter: [provider: Provider, event: KeyboardEvent];
  openCliInstances: [provider: Provider];
}>();

const toneLabels: Partial<Record<ProviderCardTone, string>> = {
  syncing: "同步",
  warning: "待签到",
  empty: "无余额",
  error: "异常",
};

const authModeLabel = computed(() => providerAuthModeLabel(props.provider));
const authModeDescription = computed(() => providerAuthModeDescription(props.provider));

function providerStatusLabel() {
  if (props.tone === "disabled") {
    return props.provider.runtime.enabled ? "待同步" : "已停用";
  }
  return toneLabels[props.tone] || "";
}

function providerLogoSrc(provider: Provider) {
  return provider.identity.siteLogo?.trim() || newApiLogo;
}

function handleProviderLogoError(event: Event) {
  const image = event.target as HTMLImageElement;
  if (image.src !== newApiLogo) {
    image.src = newApiLogo;
  }
}

function handleClick(event: MouseEvent) {
  if (props.interactive) {
    emit("click", props.provider, event);
  }
}

function handleContextMenu(event: MouseEvent) {
  if (props.interactive) {
    emit("contextmenu", props.provider, event);
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

function openCliInstances() {
  emit("openCliInstances", props.provider);
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
      },
    ]"
    :role="interactive ? 'button' : undefined"
    :aria-disabled="interactive ? !provider.runtime.enabled : undefined"
    :aria-hidden="ariaHidden || undefined"
    :tabindex="interactive ? 0 : undefined"
    :title="title"
    :style="dragStyle"
    @click="handleClick"
    @contextmenu.stop.prevent="handleContextMenu"
    @dragstart.prevent
    @pointerdown="handlePointerDown"
    @keydown.enter="handleEnter"
  >
    <div class="provider-card-header">
      <div class="provider-card-brand">
        <div class="provider-logo provider-card-logo">
          <img :src="providerLogoSrc(provider)" alt="NewAPI" draggable="false" @error="handleProviderLogoError" />
        </div>
        <h3 class="provider-card-title">{{ provider.identity.name }}</h3>
      </div>
      <span v-if="providerStatusLabel()" class="provider-card-status">
        {{ providerStatusLabel() }}
      </span>
    </div>

    <div
      v-if="providerIdentityName(provider) || providerIdentityId(provider)"
      class="provider-card-identity"
    >
      <strong
        v-if="providerIdentityName(provider)"
        class="provider-card-user-name"
        :title="providerIdentityName(provider)"
      >
        {{ providerIdentityName(provider) }}
      </strong>
      <div
        v-if="providerIdentitySecondaryUsername(provider) || providerIdentityId(provider)"
        class="provider-card-user-meta"
      >
        <span
          v-if="providerIdentitySecondaryUsername(provider)"
          class="provider-card-user-username"
          :title="providerIdentitySecondaryUsername(provider)"
        >
          {{ providerIdentitySecondaryUsername(provider) }}
        </span>
        <span
          v-if="providerIdentityId(provider)"
          class="provider-card-user-id"
          :title="providerIdentityId(provider)"
        >
          ID {{ providerIdentityId(provider) }}
        </span>
      </div>
    </div>

    <div class="provider-card-auth">
      <span class="provider-card-auth-mode" :title="authModeDescription">
        {{ authModeLabel }}
      </span>
      <div class="provider-card-cli-context">
        <span
          v-if="codexDefault"
          class="provider-card-cli-default provider-card-cli-default-codex"
          title="当前 Codex 配置文件使用此中转站"
        >
          Codex 默认
        </span>
        <span
          v-if="claudeDefault"
          class="provider-card-cli-default provider-card-cli-default-claude"
          title="当前 Claude Code 配置文件使用此中转站"
        >
          Claude 默认
        </span>
        <button
          v-if="activeCliCount > 0"
          type="button"
          class="provider-card-cli-button"
          :title="`查看 ${activeCliCount} 个临时 CLI 实例`"
          @click.stop="openCliInstances"
          @pointerdown.stop
          @keydown.enter.stop="openCliInstances"
        >
          <icon-code />
          CLI {{ activeCliCount }}
        </button>
      </div>
    </div>

    <div class="provider-card-balance">
      <span>{{ providerQuotaScopeLabel(provider) }}</span>
      <strong :title="providerAvailableQuotaLabel(provider)">
        {{ providerAvailableQuotaLabel(provider) }}
      </strong>
    </div>

    <div v-if="!providerQuotaUnlimited(provider)" class="provider-card-progress-row">
      <span>剩余 {{ availablePercentLabel(provider) }}</span>
      <a-progress
        class="provider-quota-progress"
        :percent="availablePercent(provider)"
        :show-text="false"
        size="small"
      />
    </div>
    <div v-else class="provider-card-unlimited">无限额度</div>

    <div v-if="!providerQuotaUnlimited(provider)" class="provider-card-quota-row">
      <span class="provider-card-quota-item">
        <small>已用</small>
        <strong :title="formatProviderQuota(provider, provider.quota.used)">
          {{ formatProviderQuota(provider, provider.quota.used) }}
        </strong>
      </span>
      <span class="provider-card-quota-item provider-card-quota-total">
        <small>总额</small>
        <strong :title="providerTotalQuotaLabel(provider)">
          {{ providerTotalQuotaLabel(provider) }}
        </strong>
      </span>
    </div>

    <ProviderLivenessTimeline
      v-if="showLivenessTimeline"
      :records="provider.liveness.records"
    />
  </article>
</template>
