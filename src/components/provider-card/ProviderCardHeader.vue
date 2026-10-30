<script setup lang="ts">
import CardIconButton from "../workspace-card/CardIconButton.vue";
import { computed, inject, toValue } from "vue";
import { Message } from "@arco-design/web-vue";
import { ExternalLink } from "@lucide/vue";
import { openProviderSite } from "../../api/provider-site";
import { IconCopy } from "@arco-design/web-vue/es/icon";
import ProviderApiKeySwitcher from "./ProviderApiKeySwitcher.vue";
import ProviderCardHeaderMeta from "./ProviderCardHeaderMeta.vue";
import WorkspaceCardHeader from "../workspace-card/WorkspaceCardHeader.vue";
import type { AgentCliKind, Provider, ProviderApiKeyOption } from "../../stores/providers";
import {
  maskApiKey,
  providerApiKeyLocalRemark,
  providerCardTitle,
  providerDefaultApiKeyOption,
  providerIdentityDisplayName,
  providerIdentityUsername,
  providerProtocolLabel,
  providerTransportProtocol,
  type ProviderCardTone,
} from "../../utils/provider-display";
import { effectiveProviderApiKeyOptions } from "../../utils/provider-api-key-options";
import { providerBoardSectionLabelKey } from "../../utils/provider-board-sections";
import { applyProviderLogoFallback, providerLogoSrc } from "./provider-card-logo";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    tone: ProviderCardTone;
    interactive?: boolean;
    activeCliCounts?: Partial<Record<AgentCliKind, number>>;
  }>(),
  {
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
const defaultApiKeyRemark = computed(() => {
  const option = defaultApiKey.value;
  return option ? providerApiKeyLocalRemark(option) : "";
});
/// 卡片标题已经是这条备注时，Key 行不再重复同一句话；只有 Key 的本地名称
/// 与卡片标题不同（例如标题是站点、Key 是「Claude 主用」）才值得再显示。
const showApiKeyName = computed(() => {
  const name = defaultApiKeyRemark.value;
  const title = providerHeaderTitle.value.trim();
  return Boolean(name) && name !== title;
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
/// 协议名只在它不在分组标题里出现时才写：看板把中转站按 NewAPI /
/// Sub2API / 通用 API Key 分组，副标题再写一遍只是把账号名挤窄；独立展示
/// 卡片（没有分组标题）时仍需要它说明协议。
const protocolDuplicatedBySection = inject(providerBoardSectionLabelKey, null);
const providerHeaderSubtitle = computed(() => {
  const label = providerProtocolLabel(props.provider);
  const section = toValue(protocolDuplicatedBySection) ?? "";
  return section && section.includes(label) ? "" : label;
});
/// 地址行本身已经带着 https://，再过一遍默认协议只是噪音；只有非加密地址
/// 才值得在标题行单独提醒。
const providerTransportLabel = computed(() => {
  const scheme = providerTransportProtocol(props.provider.identity.baseUrl);
  return scheme && scheme !== "HTTPS" ? scheme : "";
});
/// 账号卡把账号名提到副标题，正文就可以只放额度；用户名为空时退回用户名。
const identityLine = computed(() =>
  isApiKeyAuth.value
    ? ""
    : providerIdentityDisplayName(props.provider) ||
      providerIdentityUsername(props.provider),
);
/// 状态点在 warning 上按中转站卡自己的语义说话：这是「待签到」，只有带异常
/// 消息时才是「需关注」。
const statusWarningLabel = computed(() =>
  props.provider.runtime.errorMessage ? "需关注" : "待签到",
);

function handleProviderLogoError(event: Event) {
  applyProviderLogoFallback(event, props.provider);
}

async function openSite() {
  try {
    await openProviderSite(props.provider.identity.id);
  } catch (error) {
    Message.error(String(error));
  }
}
</script>

<template>
<WorkspaceCardHeader :title="providerHeaderTitle">
  <template #icon>
      <img
        :src="providerLogoSrc(provider)"
        :alt="providerHeaderTitle"
        draggable="false"
        @error="handleProviderLogoError"
      />
  </template>
  <template #title-actions>
        <CardIconButton
          v-if="interactive && provider.identity.baseUrl.trim()"
          tone="link"
          title="打开中转站"
          aria-label="打开中转站"
          @click.stop="openSite"
          @pointerdown.stop
          @keydown.enter.stop
        ><ExternalLink :size="15" :stroke-width="1.8" /></CardIconButton>
  </template>
  <template #subtitle>
      <!-- 副标题的位置四类卡片相同，内容由卡型的数据决定：中转站卡是协议与
           账号，Key 卡是接口地址和当前 Key 这两行。 -->
      <dl
        v-if="isApiKeyAuth"
        class="provider-card-api-summary"
        aria-label="API Key 信息"
      >
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
                <span v-if="showApiKeyName" class="provider-card-api-key-name">
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
      <!-- 账号卡：协议名（分组标题没写时）+ 账号或用户信息，标题行只留站点名。 -->
      <span
        v-else
        class="workspace-card-subtitle provider-card-header-subtitle"
      >
        <span v-if="providerHeaderSubtitle" class="provider-card-api-protocol">{{ providerHeaderSubtitle }}</span>
        <span
          v-if="providerHeaderSubtitle && providerTransportLabel"
          class="provider-card-api-protocol-separator"
          aria-hidden="true"
        >·</span>
        <span v-if="providerTransportLabel" class="provider-card-api-transport">{{ providerTransportLabel }}</span>
        <span
          v-if="identityLine"
          class="provider-card-header-identity"
          :title="identityLine"
        >{{ identityLine }}</span>
      </span>
  </template>
  <template #meta>
    <ProviderCardHeaderMeta
      :provider="provider"
      :tone="tone"
      :interactive="interactive"
      :active-cli-counts="activeCliCounts"
      :warning-label="statusWarningLabel"
      @open-cli-instances="
        (target, cliKind) => emit('openCliInstances', target, cliKind)
      "
    />
  </template>
</WorkspaceCardHeader>
</template>
