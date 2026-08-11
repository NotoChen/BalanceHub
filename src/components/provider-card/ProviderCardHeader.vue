<script setup lang="ts">
import { computed } from "vue";
import BrandIcon from "../BrandIcon.vue";
import type { LivenessCliKind, Provider } from "../../stores/providers";
import {
  maskApiKey,
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
    codexActiveCliCount?: number;
    claudeActiveCliCount?: number;
  }>(),
  {
    title: "",
    interactive: true,
    codexActiveCliCount: 0,
    claudeActiveCliCount: 0,
  },
);

const emit = defineEmits<{
  openCliInstances: [provider: Provider, cliKind: LivenessCliKind];
}>();

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const apiKeyMasked = computed(() => maskApiKey(props.provider.auth.apiKey));
const apiKeyConfigured = computed(() => Boolean(props.provider.auth.apiKey.trim()));
const providerUrlDisplay = computed(() =>
  props.provider.identity.baseUrl
    .trim()
    .replace(/^https?:\/\//i, "")
    .replace(/\/+$/, ""),
);
const providerHeaderTitle = computed(() => props.provider.identity.name);
const providerHeaderSubtitle = computed(() =>
  providerProtocolLabel(props.provider.identity.protocol),
);
const showProviderStatus = computed(() => !isApiKeyAuth.value || props.tone !== "ok");

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

function openCliInstances(cliKind: LivenessCliKind) {
  emit("openCliInstances", props.provider, cliKind);
}
</script>

<template>
<header class="provider-card-header">
  <dl v-if="isApiKeyAuth" class="provider-card-api-summary" aria-label="API Key 信息">
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
        :alt="provider.identity.name"
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
      v-if="codexActiveCliCount > 0 || claudeActiveCliCount > 0"
      class="provider-card-cli-signals"
      :class="{ 'provider-card-cli-signals-standalone': !showProviderStatus }"
      aria-label="CLI 使用状态"
    >
      <button
        v-if="codexActiveCliCount > 0 && interactive"
        type="button"
        class="provider-card-cli-signal provider-card-cli-signal-active"
        :title="`查看 ${codexActiveCliCount} 个 Codex 临时 CLI`"
        :aria-label="`查看 ${codexActiveCliCount} 个 Codex 临时 CLI`"
        @click.stop="openCliInstances('codex')"
        @pointerdown.stop
      >
        <BrandIcon brand="codex" :size="17" />
        <b>{{ codexActiveCliCount }}</b>
      </button>
      <span
        v-else-if="codexActiveCliCount > 0"
        class="provider-card-cli-signal provider-card-cli-signal-active"
        :title="`${codexActiveCliCount} 个 Codex 临时 CLI`"
        :aria-label="`${codexActiveCliCount} 个 Codex 临时 CLI`"
      >
        <BrandIcon brand="codex" :size="17" />
        <b>{{ codexActiveCliCount }}</b>
      </span>
      <button
        v-if="claudeActiveCliCount > 0 && interactive"
        type="button"
        class="provider-card-cli-signal provider-card-cli-signal-active"
        :title="`查看 ${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
        :aria-label="`查看 ${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
        @click.stop="openCliInstances('claudeCode')"
        @pointerdown.stop
      >
        <BrandIcon brand="claude" :size="17" />
        <b>{{ claudeActiveCliCount }}</b>
      </button>
      <span
        v-else-if="claudeActiveCliCount > 0"
        class="provider-card-cli-signal provider-card-cli-signal-active"
        :title="`${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
        :aria-label="`${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
      >
        <BrandIcon brand="claude" :size="17" />
        <b>{{ claudeActiveCliCount }}</b>
      </span>
    </div>
    <div v-if="showProviderStatus" class="provider-card-status" :title="title">
      <i aria-hidden="true"></i>
      <span>{{ providerStatusLabel() }}</span>
    </div>
  </div>
</header>
</template>
