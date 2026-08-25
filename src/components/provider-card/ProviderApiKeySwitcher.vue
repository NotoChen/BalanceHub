<script setup lang="ts">
import { ref } from "vue";
import { IconCheck, IconSwap } from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderApiKeyOption } from "../../stores/providers";
import {
  maskApiKey,
  providerApiKeyLocalRemark,
  providerUsesApiKeyOption,
} from "../../utils/provider-display";

const props = defineProps<{
  provider: Provider;
  options: ProviderApiKeyOption[];
}>();

const emit = defineEmits<{
  select: [option: ProviderApiKeyOption];
  manage: [];
}>();

const popupVisible = ref(false);

function apiKeyLabel(option: ProviderApiKeyOption) {
  return providerApiKeyLocalRemark(option)
    || option.maskedKey?.trim()
    || maskApiKey(option.key)
    || "未命名 Key";
}

function apiKeyMaskedLabel(option: ProviderApiKeyOption) {
  const masked = option.maskedKey?.trim() || maskApiKey(option.key);
  return masked && providerApiKeyLocalRemark(option) ? masked : "";
}

function isCurrentApiKey(option: ProviderApiKeyOption) {
  return providerUsesApiKeyOption(props.provider, option);
}

function canSelectApiKey(option: ProviderApiKeyOption) {
  return Boolean(option.localId.trim() && option.keyAvailable && option.key.trim());
}

function selectApiKey(option: ProviderApiKeyOption) {
  if (!canSelectApiKey(option)) return;
  popupVisible.value = false;
  if (!isCurrentApiKey(option)) {
    emit("select", option);
  }
}

function openApiKeyManager() {
  popupVisible.value = false;
  emit("manage");
}
</script>

<template>
  <a-popover
    v-model:popup-visible="popupVisible"
    trigger="click"
    position="rb"
    content-class="provider-card-api-key-popover"
  >
    <button
      type="button"
      class="provider-card-api-key-switcher-trigger"
      title="切换当前 API Key"
      aria-label="切换当前 API Key"
      @click.stop
      @pointerdown.stop
    >
      <IconSwap />
      <span v-if="options.length > 1" class="provider-card-api-key-count-badge">
        {{ options.length }}
      </span>
    </button>
    <template #content>
      <div class="provider-card-api-key-switcher" @click.stop @pointerdown.stop>
        <header class="provider-card-api-key-switcher-header">
          <strong>切换当前 Key</strong>
          <span>{{ options.length }} 个</span>
        </header>
        <div class="provider-card-api-key-switcher-list">
          <button
            v-for="option in options"
            :key="option.localId || option.tokenId || option.key"
            type="button"
            class="provider-card-api-key-switcher-option"
            :class="{ current: isCurrentApiKey(option) }"
            :disabled="!canSelectApiKey(option)"
            :title="canSelectApiKey(option) ? `切换到${apiKeyLabel(option)}` : '此 Key 无法在本机切换'"
            @click="selectApiKey(option)"
          >
            <span class="provider-card-api-key-switcher-check" aria-hidden="true">
              <IconCheck v-if="isCurrentApiKey(option)" />
            </span>
            <span class="provider-card-api-key-switcher-copy">
              <strong>{{ apiKeyLabel(option) }}</strong>
              <small v-if="apiKeyMaskedLabel(option)">{{ apiKeyMaskedLabel(option) }}</small>
            </span>
          </button>
        </div>
        <button
          type="button"
          class="provider-card-api-key-switcher-manage"
          @click="openApiKeyManager"
        >
          管理 API Key
        </button>
      </div>
    </template>
  </a-popover>
</template>
