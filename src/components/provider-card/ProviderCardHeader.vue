<script setup lang="ts">
import { computed } from "vue";
import { IconCopy } from "@arco-design/web-vue/es/icon";
import AgentCliIcon from "../AgentCliIcon.vue";
import ProviderApiKeySwitcher from "./ProviderApiKeySwitcher.vue";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentCliKind, Provider, ProviderApiKeyOption } from "../../stores/providers";
import { agentCliLabel } from "../../utils/cli-environment";
import {
  maskApiKey,
  providerApiKeyLocalRemark,
  providerApiKeyRemark,
  providerCardTitle,
  providerDefaultApiKeyOption,
  providerProtocolLabel,
  providerTransportProtocol,
  type ProviderCardTone,
} from "../../utils/provider-display";
import { effectiveProviderApiKeyOptions } from "../../utils/provider-api-key-options";
import { applyProviderLogoFallback, providerLogoSrc } from "./provider-card-logo";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    tone: ProviderCardTone;
    title?: string;
    interactive?: boolean;
    activeCliCounts?: Partial<Record<AgentCliKind, number>>;
  }>(),
  {
    title: "",
    interactive: true,
    activeCliCounts: () => ({}),
  },
);

const emit = defineEmits<{
  openCliInstances: [provider: Provider, cliKind: AgentCliKind];
  copyApiKey: [provider: Provider];
  manageApiKeys: [provider: Provider];
  selectApiKey: [provider: Provider, option: ProviderApiKeyOption];
}>();
const store = useCliRuntimeStore();

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const defaultApiKey = computed(() => providerDefaultApiKeyOption(props.provider));
const apiKeyMasked = computed(() =>
  maskApiKey(defaultApiKey.value?.key || props.provider.auth.apiKey),
);
const apiKeyConfigured = computed(() => Boolean(defaultApiKey.value?.key.trim() || props.provider.auth.apiKey.trim()));
const providerUrlDisplay = computed(() =>
  props.provider.identity.baseUrl
    .trim()
    .replace(/\/+$/, ""),
);
const providerHeaderTitle = computed(() => providerCardTitle(props.provider));
const apiKeyRemark = computed(() => providerApiKeyRemark(props.provider));
const defaultApiKeyRemark = computed(() => {
  const option = defaultApiKey.value;
  return option ? providerApiKeyLocalRemark(option) : "";
});
const apiKeyCount = computed(() => props.provider.auth.apiKeyOptions.length || (props.provider.auth.apiKey.trim() ? 1 : 0));
const apiKeyOptions = computed(() =>
  effectiveProviderApiKeyOptions(
    props.provider.auth.apiKey,
    props.provider.auth.apiKeyOptions || [],
  ),
);
const apiKeySelectionHint = computed(() =>
  !defaultApiKey.value && apiKeyCount.value > 1 ? "请先选择" : "",
);
const providerHeaderSubtitle = computed(() =>
  providerProtocolLabel(props.provider),
);
const providerTransportLabel = computed(() =>
  providerTransportProtocol(props.provider.identity.baseUrl),
);
const showProviderStatus = computed(() => !isApiKeyAuth.value || props.tone !== "ok");
const activeCliSignals = computed(() =>
  Object.entries(props.activeCliCounts)
    .filter((entry): entry is [AgentCliKind, number] => Number(entry[1]) > 0)
    .map(([cliKind, count]) => ({
      cliKind,
      count,
      label: agentCliLabel(store.cliEnvironmentProbe, cliKind),
    })),
);

const toneLabels: Record<Exclude<ProviderCardTone, "disabled">, string> = {
  ok: "正常",
  pending: "待同步",
  syncing: "同步中",
  warning: "待签到",
  empty: "无余额",
  error: "异常",
};

function providerStatusLabel() {
  return props.tone === "disabled" ? "已停用" : toneLabels[props.tone];
}

function handleProviderLogoError(event: Event) {
  applyProviderLogoFallback(event, props.provider);
}

function openCliInstances(cliKind: AgentCliKind) {
  emit("openCliInstances", props.provider, cliKind);
}
</script>

<template>
<header class="provider-card-header">
  <dl v-if="isApiKeyAuth" class="provider-card-api-summary" aria-label="API Key 信息">
    <div class="provider-card-api-heading">
      <div v-if="apiKeyRemark" class="provider-card-api-remark" :title="apiKeyRemark">
        {{ apiKeyRemark }}
      </div>
      <span class="provider-card-api-protocol-group" aria-label="协议和站点类型">
        <span v-if="providerTransportLabel" class="provider-card-api-transport">
          {{ providerTransportLabel }}
        </span>
        <span v-if="providerTransportLabel" class="provider-card-api-protocol-separator" aria-hidden="true">·</span>
        <span class="provider-card-api-protocol">{{ providerHeaderSubtitle }}</span>
      </span>
    </div>
    <div class="provider-card-api-endpoint-row" aria-label="地址">
      <div
        v-if="provider.identity.baseUrl.trim()"
        class="provider-card-api-endpoint-value"
        :title="provider.identity.baseUrl"
      >
        <span class="provider-card-api-endpoint-text">{{ providerUrlDisplay }}</span>
      </div>
      <div v-else class="provider-card-api-value-muted">未配置</div>
    </div>
    <div class="provider-card-api-key-row" aria-label="当前 API Key">
        <span
          class="provider-card-api-key-value"
          :title="defaultApiKeyRemark || (apiKeyMasked ? `API Key：${apiKeyMasked}` : 'API Key 未配置')"
        >
          <span v-if="apiKeySelectionHint" class="provider-card-api-key-selection-hint">
            {{ apiKeySelectionHint }}
          </span>
          <template v-else>
            <span v-if="defaultApiKeyRemark" class="provider-card-api-key-name">
              {{ defaultApiKeyRemark }}
            </span>
            <code v-if="apiKeyConfigured" :title="`API Key：${apiKeyMasked}`">
              {{ apiKeyMasked }}
            </code>
            <span v-if="!apiKeyConfigured" class="provider-card-api-value-muted">未配置</span>
          </template>
        </span>
        <span v-if="interactive" class="provider-card-api-key-inline-actions">
          <ProviderApiKeySwitcher
            :provider="provider"
            :options="apiKeyOptions"
            @select="emit('selectApiKey', provider, $event)"
            @manage="emit('manageApiKeys', provider)"
          />
          <button
            v-if="apiKeyConfigured"
            type="button"
            title="复制当前 API Key"
            aria-label="复制当前 API Key"
            @click.stop="emit('copyApiKey', provider)"
            @pointerdown.stop
          >
            <IconCopy />
          </button>
        </span>
    </div>
  </dl>
  <div v-else class="provider-card-brand">
    <div class="provider-logo provider-card-logo">
      <img
        :src="providerLogoSrc(provider)"
        :alt="providerHeaderTitle"
        draggable="false"
        @error="handleProviderLogoError"
      />
    </div>
    <div class="provider-card-brand-copy">
      <h3 class="provider-card-title" :title="providerHeaderTitle">
        {{ providerHeaderTitle }}
      </h3>
      <span class="provider-card-type">
        {{ providerHeaderSubtitle }}
      </span>
    </div>
  </div>
  <div class="provider-card-header-meta">
    <div
      v-if="activeCliSignals.length > 0"
      class="provider-card-cli-signals"
      :class="{ 'provider-card-cli-signals-standalone': !showProviderStatus }"
      aria-label="CLI 使用状态"
    >
      <template v-for="signal in activeCliSignals" :key="signal.cliKind">
        <button
          v-if="interactive"
          type="button"
          class="provider-card-cli-signal provider-card-cli-signal-active"
          :title="`查看 ${signal.count} 个 ${signal.label} 临时 CLI`"
          :aria-label="`查看 ${signal.count} 个 ${signal.label} 临时 CLI`"
          @click.stop="openCliInstances(signal.cliKind)"
          @pointerdown.stop
        >
          <AgentCliIcon :kind="signal.cliKind" :size="17" />
          <b>{{ signal.count }}</b>
        </button>
        <span
          v-else
          class="provider-card-cli-signal provider-card-cli-signal-active"
          :title="`${signal.count} 个 ${signal.label} 临时 CLI`"
          :aria-label="`${signal.count} 个 ${signal.label} 临时 CLI`"
        >
          <AgentCliIcon :kind="signal.cliKind" :size="17" />
          <b>{{ signal.count }}</b>
        </span>
      </template>
    </div>
    <div v-if="showProviderStatus" class="provider-card-status" :title="title">
      <i aria-hidden="true"></i>
      <span>{{ providerStatusLabel() }}</span>
    </div>
  </div>
</header>
</template>
