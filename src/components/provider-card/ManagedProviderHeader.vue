<script setup lang="ts">
import { computed } from "vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import ProviderCardHeaderMeta from "./ProviderCardHeaderMeta.vue";
import WorkspaceCardHeader from "../workspace-card/WorkspaceCardHeader.vue";
import cpaLogo from "../../assets/logos/cli-proxy-api.png";
import type { AgentCliKind } from "../../stores/providers";
import type { Provider } from "../../stores/provider-types";
import {
  providerCardTitle,
  type ProviderCardTone,
} from "../../utils/provider-display";
import {
  oauthVendorLabel,
  planLabel,
} from "../../utils/oauth-display";
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
}>();
const oauth = computed(() => props.provider.auth.oauth);
const label = computed(() =>
  oauth.value ? oauthVendorLabel(oauth.value.vendor) : "CPA",
);
/// 标题和中转站卡片同一份规则：本地备注也算在名字里。没有名字时才退回
/// 厂商名，卡片不至于只剩一枚图标。
const cardTitle = computed(() => providerCardTitle(props.provider) || label.value);
/// The gateway address is already an implementation detail; the host is the
/// part that tells two gateways apart at a glance.
const gatewayHost = computed(() => {
  try {
    return new URL(props.provider.identity.baseUrl).host;
  } catch {
    return props.provider.identity.baseUrl;
  }
});
const plan = computed(() =>
  planLabel(
    props.provider.capabilities.subscription?.plan || oauth.value?.plan || "",
  ),
);
</script>
<template>
  <!-- Vendor marks are artwork, not badges: the OAuth logos are transparent and
       the gateway mark carries its own tile, so neither gets a frame behind it. -->
  <WorkspaceCardHeader :title="cardTitle">
    <template #icon
      ><AgentCliIcon v-if="oauth" :kind="oauth.vendor" :size="30" /><img
        v-else
        class="brand-icon cpa-gateway-mark"
        :src="cpaLogo"
        width="34"
        height="34"
        alt=""
        aria-hidden="true"
        draggable="false"
    /></template>
    <template #subtitle
      ><span
        class="managed-card-subtitle"
        :title="oauth?.email || provider.identity.baseUrl"
        >{{ oauth?.email || (oauth ? label : gatewayHost) }}</span
      ></template
    >
    <template #meta>
      <div class="managed-card-badges">
        <ProviderCardHeaderMeta
          :provider="provider"
          :tone="tone"
          :interactive="interactive"
          :active-cli-counts="activeCliCounts"
          @open-cli-instances="
            (target, cliKind) => emit('openCliInstances', target, cliKind)
          "
        />
        <span v-if="plan" class="managed-card-plan" :title="`套餐：${plan}`">{{
          plan
        }}</span>
      </div>
    </template>
  </WorkspaceCardHeader>
</template>
<style scoped>
.managed-card-subtitle {
  display: block;
  color: var(--color-text-3);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 头部右侧是一列：状态点始终在标题首行，和其余三类卡片同一高度；套餐徽标
   挂在它下面。横排会把标题挤掉一截，而标题才是这张卡的名字。 */
.managed-card-badges {
  display: grid;
  justify-items: end;
  gap: 4px;
  flex-shrink: 0;
  max-width: 128px;
}
.managed-card-plan {
  max-width: 128px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border: 1px solid var(--managed-accent-line, var(--color-border-2));
  border-radius: 999px;
  background: var(--managed-accent-soft, transparent);
  color: var(--managed-accent-ink, var(--color-text-2));
  font-size: 10px;
  font-weight: 550;
  letter-spacing: 0.01em;
  line-height: 16px;
  padding: 0 8px;
}
</style>
