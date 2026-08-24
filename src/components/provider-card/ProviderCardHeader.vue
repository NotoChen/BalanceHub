<script setup lang="ts">
import { computed } from "vue";
import { IconCopy, IconSettings } from "@arco-design/web-vue/es/icon";
import AgentCliIcon from "../AgentCliIcon.vue";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentCliKind, Provider } from "../../stores/providers";
import { agentCliLabel } from "../../utils/cli-environment";
import {
  maskApiKey,
  providerApiKeyCardName,
  providerApiKeyRemark,
  providerCardTitle,
  providerDefaultApiKeyOption,
  providerProtocolLabel,
  type ProviderCardTone,
} from "../../utils/provider-display";
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
    .replace(/^https?:\/\//i, "")
    .replace(/\/+$/, ""),
);
const providerHeaderTitle = computed(() => providerCardTitle(props.provider));
const apiKeyRemark = computed(() => providerApiKeyRemark(props.provider));
const defaultApiKeyLabel = computed(() => {
  const option = defaultApiKey.value;
  return option ? providerApiKeyCardName(option) : "";
});
const apiKeyCount = computed(() => props.provider.auth.apiKeyOptions.length || (props.provider.auth.apiKey.trim() ? 1 : 0));
const apiKeySummaryLabel = computed(() =>
  apiKeyCount.value > 1 ? `当前调用 · 共 ${apiKeyCount.value} 把` : "当前调用",
);
const apiKeySelectionHint = computed(() =>
  !defaultApiKey.value && apiKeyCount.value > 1 ? "请先选择" : "",
);
const providerHeaderSubtitle = computed(() =>
  providerProtocolLabel(props.provider),
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
    <div v-if="apiKeyRemark" class="provider-card-api-remark" :title="apiKeyRemark">
      {{ apiKeyRemark }}
    </div>
    <div class="provider-card-api-field">
      <dt>接口地址</dt>
      <dd
        v-if="provider.identity.baseUrl.trim()"
        class="provider-card-api-endpoint-value"
        :title="provider.identity.baseUrl"
      >
        {{ providerUrlDisplay }}
      </dd>
      <dd v-else class="provider-card-api-value-muted">未配置</dd>
    </div>
    <div class="provider-card-api-field">
      <dt>{{ apiKeySummaryLabel }}</dt>
      <dd class="provider-card-api-key-row">
        <span
          class="provider-card-api-key-value"
          :title="defaultApiKeyLabel ? `${defaultApiKeyLabel} · ${apiKeyMasked}` : apiKeyMasked"
        >
          <span v-if="defaultApiKeyLabel" class="provider-card-api-key-name">
            {{ defaultApiKeyLabel }}
          </span>
          <span v-if="apiKeySelectionHint" class="provider-card-api-key-selection-hint">
            {{ apiKeySelectionHint }}
          </span>
          <code v-if="apiKeyConfigured && !apiKeySelectionHint" :title="`API Key：${apiKeyMasked}`">
            {{ apiKeyMasked }}
          </code>
          <span v-else-if="!apiKeySelectionHint" class="provider-card-api-value-muted">未配置</span>
        </span>
        <span v-if="interactive" class="provider-card-api-key-inline-actions">
          <button
            type="button"
            title="管理 API Key 与调用配置"
            aria-label="管理 API Key 与调用配置"
            @click.stop="emit('manageApiKeys', provider)"
            @pointerdown.stop
          >
            <IconSettings />
          </button>
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
      </dd>
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
