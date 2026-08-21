<script setup lang="ts">
import { computed } from "vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentCliKind, Provider } from "../../stores/providers";
import { agentCliLabel } from "../../utils/cli-environment";
import {
  maskApiKey,
  providerApiKeyDisplayName,
  providerApiKeyRemark,
  providerApiKeySecondaryName,
  providerCardTitle,
  providerPrimaryApiKeyOption,
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
}>();
const store = useCliRuntimeStore();

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const apiKeyMasked = computed(() => maskApiKey(props.provider.auth.apiKey));
const apiKeyConfigured = computed(() => Boolean(props.provider.auth.apiKey.trim()));
const providerUrlDisplay = computed(() =>
  props.provider.identity.baseUrl
    .trim()
    .replace(/^https?:\/\//i, "")
    .replace(/\/+$/, ""),
);
const providerHeaderTitle = computed(() => providerCardTitle(props.provider));
const apiKeyRemark = computed(() => providerApiKeyRemark(props.provider));
const primaryApiKey = computed(() => providerPrimaryApiKeyOption(props.provider));
const primaryApiKeyName = computed(() =>
  primaryApiKey.value
    ? providerApiKeyDisplayName(primaryApiKey.value)
    : apiKeyConfigured.value
      ? "当前配置 API Key"
      : "",
);
const primaryApiKeySecondaryName = computed(() =>
  primaryApiKey.value ? providerApiKeySecondaryName(primaryApiKey.value) : "",
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
      <dt>主 Key</dt>
      <dd v-if="primaryApiKeyName" class="provider-card-api-primary-name" :title="primaryApiKeyName">
        {{ primaryApiKeyName }}
        <span v-if="primaryApiKeySecondaryName"> · {{ primaryApiKeySecondaryName }}</span>
      </dd>
      <dd v-else class="provider-card-api-value-muted">未选择</dd>
    </div>
    <div class="provider-card-api-field">
      <dt>API Key</dt>
      <dd class="provider-card-api-key-value">
        <code v-if="apiKeyConfigured" :title="`API Key：${apiKeyMasked}`">
          {{ apiKeyMasked }}
        </code>
        <span v-else class="provider-card-api-value-muted">未配置</span>
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
