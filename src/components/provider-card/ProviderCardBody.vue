<script setup lang="ts">
import { computed } from "vue";
import ProviderLivenessTimeline from "../ProviderLivenessTimeline.vue";
import ProviderModelPreview from "../ProviderModelPreview.vue";
import type { Provider } from "../../stores/providers";
import {
  availablePercent,
  availablePercentLabel,
  providerAvailableQuotaLabel,
  providerIdentityDisplayName,
  providerIdentityId,
  providerIdentitySecondaryUsername,
  providerIdentityUsername,
  providerQuotaKnown,
  providerQuotaScopeLabel,
  providerQuotaTotalKnown,
  providerQuotaUnlimited,
  formatProviderSyncTime,
} from "../../utils/provider-display";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    showLivenessTimeline?: boolean;
  }>(),
  {
    showLivenessTimeline: false,
  },
);

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const identityDisplayName = computed(
  () => providerIdentityDisplayName(props.provider) || providerIdentityUsername(props.provider),
);
const identityUsername = computed(() => providerIdentitySecondaryUsername(props.provider));
const identityId = computed(() => providerIdentityId(props.provider));
const identityEmptyLabel = computed(() =>
  props.provider.identity.protocol === "api" ? "通用模型接口" : "用户信息未同步",
);
const modelSyncTime = computed(() =>
  formatProviderSyncTime(props.provider.automation.lastSyncedAt),
);
const quotaTone = computed(() => {
  if (providerQuotaUnlimited(props.provider)) {
    return "unlimited";
  }
  if (!props.provider.automation.lastSyncedAt) {
    return "neutral";
  }
  const percent = availablePercent(props.provider);
  if (props.provider.quota.available <= 0 || percent <= 0) {
    return "empty";
  }
  return percent <= 0.2 ? "warning" : "normal";
});
</script>

<template>
<section
  v-if="!isApiKeyAuth"
  class="provider-card-identity"
  aria-label="账号信息"
>
  <strong
    v-if="identityDisplayName"
    class="provider-card-user-name"
    :title="identityDisplayName"
  >
    {{ identityDisplayName }}
  </strong>
  <span v-else class="provider-card-user-name provider-card-user-name-muted">
    {{ identityEmptyLabel }}
  </span>
  <div v-if="identityUsername || identityId" class="provider-card-user-meta">
    <span v-if="identityUsername" :title="identityUsername">{{ identityUsername }}</span>
    <span v-if="identityId" :title="identityId">{{ identityId }}</span>
  </div>
</section>

<section
  v-if="!isApiKeyAuth"
  class="provider-card-quota"
  :class="`provider-card-quota-${quotaTone}`"
  aria-label="账户余额"
>
  <div class="provider-card-balance">
    <span>{{ providerQuotaScopeLabel(provider) }}</span>
    <strong :title="providerAvailableQuotaLabel(provider)">
      {{ providerAvailableQuotaLabel(provider) }}
    </strong>
  </div>
  <div v-if="providerQuotaKnown(provider) && providerQuotaTotalKnown(provider) && !providerQuotaUnlimited(provider)" class="provider-card-progress-row">
    <span>可用 {{ availablePercentLabel(provider) }}</span>
    <a-progress
      class="provider-quota-progress"
      :percent="availablePercent(provider)"
      :show-text="false"
      size="small"
    />
  </div>
  <div v-else-if="providerQuotaUnlimited(provider)" class="provider-card-unlimited">无限额度</div>
  <div v-else class="provider-card-unknown">额度未公开</div>
</section>

<ProviderModelPreview
  :models="provider.capabilities.availableModels"
  :rows="isApiKeyAuth ? 5 : 2"
  :sync-time="isApiKeyAuth ? modelSyncTime : ''"
/>

<ProviderLivenessTimeline
  v-if="showLivenessTimeline"
  :records="provider.liveness.records"
/>
</template>
