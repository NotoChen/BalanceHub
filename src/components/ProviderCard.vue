<script setup lang="ts">
import type { CSSProperties } from "vue";
import ProviderLivenessTimeline from "./ProviderLivenessTimeline.vue";
import type { Provider } from "../stores/providers";
import {
  formatProviderQuota,
  providerAvailableQuotaLabel,
  providerIdentityDisplayName,
  providerIdentityId,
  providerIdentityName,
  providerIdentityUsername,
  providerQuotaScopeLabel,
  providerTotalQuotaLabel,
  quotaPercent,
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
    ariaHidden: false,
  },
);

const emit = defineEmits<{
  click: [provider: Provider, event: MouseEvent];
  contextmenu: [provider: Provider, event: MouseEvent];
  pointerdown: [provider: Provider, event: PointerEvent];
  enter: [provider: Provider, event: KeyboardEvent];
}>();

const toneLabels: Partial<Record<ProviderCardTone, string>> = {
  syncing: "同步",
  warning: "签到",
  empty: "空额",
  error: "异常",
};

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
    <span v-if="toneLabels[tone]" class="provider-card-badge">{{ toneLabels[tone] }}</span>

    <div class="provider-card-header">
      <div class="provider-card-brand">
        <div class="provider-logo provider-card-logo">
          <img :src="providerLogoSrc(provider)" alt="NewAPI" draggable="false" @error="handleProviderLogoError" />
        </div>
        <h3 class="provider-card-title">{{ provider.identity.name }}</h3>
      </div>
      <div class="provider-card-user">
        <div
          v-if="providerIdentityName(provider) || providerIdentityId(provider)"
          class="provider-card-user-info"
        >
          <strong
            v-if="providerIdentityName(provider)"
            class="provider-card-user-name"
            :title="providerIdentityName(provider)"
          >
            {{ providerIdentityName(provider) }}
          </strong>
          <div
            v-if="
              (providerIdentityDisplayName(provider) && providerIdentityUsername(provider)) ||
              providerIdentityId(provider)
            "
            class="provider-card-user-meta"
          >
            <span
              v-if="providerIdentityDisplayName(provider) && providerIdentityUsername(provider)"
              class="provider-card-user-username"
              :title="providerIdentityUsername(provider)"
            >
              @{{ providerIdentityUsername(provider) }}
            </span>
            <span
              v-if="providerIdentityId(provider)"
              class="provider-card-user-id"
              :title="providerIdentityId(provider)"
            >
              ID: {{ providerIdentityId(provider) }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <div class="provider-card-total">
      <span class="quota-percent">{{ providerQuotaScopeLabel(provider) }}</span>
      <strong>{{ providerTotalQuotaLabel(provider) }}</strong>
    </div>

    <a-progress class="provider-quota-progress" :percent="quotaPercent(provider)" :show-text="false" size="small" />

    <div class="provider-card-quota-row">
      <span class="quota-used">{{ formatProviderQuota(provider, provider.quota.used) }}</span>
      <strong class="quota-available">{{ providerAvailableQuotaLabel(provider) }}</strong>
    </div>

    <ProviderLivenessTimeline
      v-if="showLivenessTimeline"
      :records="provider.liveness.records"
    />
  </article>
</template>
