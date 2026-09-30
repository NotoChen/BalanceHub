<script setup lang="ts">
import { computed } from "vue";
import { ChevronRight } from "@lucide/vue";
import type { Provider } from "../../stores/provider-types";
import {
  maskApiKey, providerApiKeyLocalRemark, providerApiKeyRemark, providerDefaultApiKeyOption,
  providerDisplayLabel, providerIdentityId, providerIdentityName, providerIdentitySecondaryUsername,
  providerIdentityUsername, providerProtocolLabel, providerSafeDisplayText, providerSiteAddressLabel,
} from "../../utils/provider-display";

const props = defineProps<{ providers: readonly Provider[]; selectedId?: string }>();
const emit = defineEmits<{ select: [providerId: string] }>();
const choices = computed(() => props.providers.map((provider) => {
  const isApiKey = provider.auth.mode === "apiKey";
  const key = isApiKey ? providerDefaultApiKeyOption(provider) : undefined;
  const username = isApiKey ? "" : providerIdentityUsername(provider) || provider.auth.loginUsername.trim();
  const accountName = isApiKey ? "" : providerIdentityName(provider) || username;
  const secondaryUsername = isApiKey ? "" : providerIdentitySecondaryUsername(provider)
    || (username.toLocaleLowerCase() !== accountName.toLocaleLowerCase() ? username : "");
  const keyRemark = isApiKey ? (key ? providerApiKeyLocalRemark(key) : "") || providerApiKeyRemark(provider) : "";
  return {
    id: provider.identity.id,
    name: providerSafeDisplayText(provider, providerDisplayLabel(provider)) || "未命名中转站",
    protocol: providerSafeDisplayText(provider, providerProtocolLabel(provider)),
    site: providerSiteAddressLabel(provider), isApiKey,
    accountName: providerSafeDisplayText(provider, accountName),
    username: providerSafeDisplayText(provider, secondaryUsername),
    accountId: isApiKey ? "" : providerSafeDisplayText(provider, providerIdentityId(provider)),
    keyRemark: providerSafeDisplayText(provider, keyRemark),
    maskedKey: isApiKey ? providerSafeDisplayText(provider, maskApiKey(key?.key.trim() || provider.auth.apiKey.trim() || key?.maskedKey.trim() || "")) : "",
  };
}));
</script>

<template>
  <ul class="agent-launch-provider-list" aria-label="选择启动使用的中转站">
    <li v-for="choice in choices" :key="choice.id">
      <button type="button" class="agent-launch-provider-option" :class="{ 'is-selected': selectedId === choice.id }" :aria-pressed="selectedId === undefined ? undefined : selectedId === choice.id" @click="emit('select', choice.id)">
        <span class="agent-launch-provider-copy">
          <span class="agent-launch-provider-heading"><strong :title="choice.name">{{ choice.name }}</strong><span class="agent-launch-provider-protocol" :title="choice.protocol">{{ choice.protocol }}</span></span>
          <span class="agent-launch-provider-address" :title="choice.site">{{ choice.site }}</span>
          <span v-if="choice.isApiKey" class="agent-launch-provider-identity"><span class="agent-launch-provider-label">API Key</span><strong v-if="choice.keyRemark" :title="choice.keyRemark">{{ choice.keyRemark }}</strong><code v-if="choice.maskedKey" :title="choice.maskedKey">{{ choice.maskedKey }}</code><span v-else class="agent-launch-provider-muted">未配置</span></span>
          <span v-else class="agent-launch-provider-identity"><span class="agent-launch-provider-label">账号</span><strong v-if="choice.accountName" :title="choice.accountName">{{ choice.accountName }}</strong><span v-else class="agent-launch-provider-muted">用户信息未同步</span><span v-if="choice.username" :title="choice.username">用户名：{{ choice.username }}</span><span v-if="choice.accountId" :title="`ID：${choice.accountId}`">ID：{{ choice.accountId }}</span></span>
        </span>
        <ChevronRight class="agent-launch-provider-arrow" :size="18" aria-hidden="true" />
      </button>
    </li>
  </ul>
</template>
