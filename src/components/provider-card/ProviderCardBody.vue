<script setup lang="ts">
import { computed } from "vue";
import ProviderLivenessTimeline from "../ProviderLivenessTimeline.vue";
import ProviderModelPreview from "../ProviderModelPreview.vue";
import type { Provider } from "../../stores/providers";
import { providerLivenessRecords, type LivenessSettings } from "../../utils/provider-liveness";
import AccountQuotaBar from "../oauth/AccountQuotaBar.vue";
import ProviderCardCooldownNote from "./ProviderCardCooldownNote.vue";
import ProviderCardEndpointNote from "./ProviderCardEndpointNote.vue";
import ProviderCardStaleNote from "./ProviderCardStaleNote.vue";
import ProviderCardPricingNote from "./ProviderCardPricingNote.vue";
import {
  formatProviderSyncTime,
  providerIdentityId,
  providerIdentitySecondaryUsername,
  providerQuotaBarWindow,
  providerQuotaHeadline,
  providerQuotaKnown,
  providerQuotaTone,
  providerQuotaTotalKnown,
  providerQuotaUnlimited,
  providerQuotaWindowOverflow,
  providerQuotaWindowRows,
  usedPercent,
  usedPercentLabel,
  type ProviderQuotaHeadline,
} from "../../utils/provider-display";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    /// 解析测活目标所需的全局设置；列表里的宿主都会传入，缺省时只按卡片声明。
    settings?: LivenessSettings;
    showLivenessTimeline?: boolean;
    interactive?: boolean;
    list?: boolean;
  }>(),
  {
    showLivenessTimeline: false,
    interactive: true,
    list: false,
  },
);
const emit = defineEmits<{ models: [] }>();

const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
/// 测活线只统计当前 Agent + 模型 + 地址的组合；换目标后旧线路的成败不再
/// 混进这一行。卡片声明为空时，目标仍由全局设置决定，不能把其他模型的
/// 记录混进来。其他目标的历史仍在记录里，测活明细可以查看。
const livenessRecords = computed(() =>
  providerLivenessRecords(props.provider, props.settings ?? null),
);
const livenessEmptyLabel = computed(() =>
  (props.provider.liveness.records?.length ?? 0) > 0 ? "当前目标暂无自动记录" : undefined,
);
const modelSyncTime = computed(() =>
  formatProviderSyncTime(props.provider.capabilities.availableModelsState.updatedAt),
);
const identityUsername = computed(() => providerIdentitySecondaryUsername(props.provider));
const identityId = computed(() => providerIdentityId(props.provider));
/// 额度区块对两类卡片都在：通用 API Key 没有额度接口时写「未公开」，额度
/// 不受限时写「不限量」——和中转站卡片同一句话，卡型不决定这里显不显示。
const quotaTone = computed(() => providerQuotaTone(props.provider));
/// A provider that has never completed a refresh has no balance to report;
/// printing 0 would read as "the account is empty".
const quotaSynced = computed(() => Boolean(props.provider.automation.lastSyncedAt));
/// 从未同步过时主数字不该写成 0（会被读成账号已经空了），也不该挂一个
/// 「账号可用额度」的标签：一张卡只有一个数字时，标签只是把「未同步」这三个
/// 字讲两遍。其余读数由 providerQuotaHeadline 统一给出（含未公开与不限量）。
const quotaHeadline = computed<ProviderQuotaHeadline>(() => {
  if (!quotaSynced.value) {
    return { label: "", value: "未同步", title: "还没有完成第一次额度查询" };
  }
  return providerQuotaHeadline(props.provider);
});
const quotaProgressVisible = computed(
  () =>
    quotaSynced.value &&
    providerQuotaKnown(props.provider) &&
    providerQuotaTotalKnown(props.provider) &&
    !providerQuotaUnlimited(props.provider),
);
/// 订阅窗口比钱包可用比例更接近“还能用多少”，所以有窗口时由周期进度条顶替
/// 钱包比例条。窗口条直接复用 OAuth 卡片的 AccountQuotaBar，账号卡、CPA 卡和
/// 中转站卡的窗口读法只有一套。不限量窗口也要列出来，否则卡片会退回钱包
/// 余额，读起来像两套额度制度。列表行只有固定高度，窗口收口到两条，其余由
/// 摘要说明，给三行模型留出空间。
const listWindowLimit = computed(() => (props.list ? 2 : 3));
const quotaWindowRows = computed(() =>
  providerQuotaWindowRows(props.provider, Date.now(), listWindowLimit.value),
);
const quotaWindowOverflow = computed(() =>
  providerQuotaWindowOverflow(props.provider, listWindowLimit.value),
);
</script>

<template>
<!-- 正文自己是一列：卡片被拉高时，多余的高度落在额度区块和模型行之间，
     模型行紧贴页脚分界线，和中转站卡片保持一致。 -->
<div class="provider-card-body">
<!-- 列表布局把身份和额度收进左列，右列留给三行模型；卡片布局里这个容器
     不生成盒子（display: contents），下面的元素仍是正文的直接子项。 -->
<div class="provider-card-body-main">
<!-- 账号名已经在副标题里，正文不再重复；这里只留用户名 / 用户 ID。 -->
<section
  v-if="identityUsername || identityId"
  class="provider-card-identity"
  aria-label="账号信息"
>
  <div class="provider-card-user-meta">
    <span v-if="identityUsername" :title="identityUsername">{{ identityUsername }}</span>
    <span v-if="identityId" :title="identityId">{{ identityId }}</span>
  </div>
</section>

<section
  class="provider-card-quota"
  :class="[
    `provider-card-quota-${quotaTone}`,
    { 'provider-card-quota-compact': isApiKeyAuth },
  ]"
  :aria-label="isApiKeyAuth ? 'API Key 额度' : '账户余额'"
>
  <div class="provider-card-balance">
    <span v-if="quotaHeadline.label" class="provider-card-balance-labels">
      <span>{{ quotaHeadline.label }}</span>
    </span>
    <strong :title="quotaHeadline.title">
      {{ quotaHeadline.value }}
    </strong>
  </div>
  <!-- 窗口条复用 OAuth 卡片的同一组件：标签与已用比例一行，整条进度条在下，
       重置时间在条下，和中转站卡片的钱包比例条同一排法。 -->
  <div v-if="quotaWindowRows.length" class="provider-card-quota-windows">
    <AccountQuotaBar
      v-for="row in quotaWindowRows"
      :key="row.label"
      :window="providerQuotaBarWindow(row)"
      compact
    />
    <span
      v-if="quotaWindowOverflow.count > 0"
      class="provider-card-quota-overflow"
      :title="quotaWindowOverflow.labels.join('、')"
      >另有 {{ quotaWindowOverflow.count }} 个额度窗口</span
    >
  </div>
  <!-- 钱包余额的比例条和窗口条同一排法：条独占一行，比例贴在条左端。 -->
  <div v-else-if="quotaProgressVisible" class="provider-card-progress-row">
    <span class="provider-card-quota-meter"
      >{{ usedPercentLabel(provider) }}<small>已用</small></span
    >
    <a-progress
      class="provider-quota-progress"
      :percent="usedPercent(provider)"
      :show-text="false"
      size="small"
    />
  </div>
  <!-- 数据新鲜时卡片不写同步时间；过期了才说一句，避免用户拿旧数字做判断。 -->
  <ProviderCardStaleNote :synced-at="provider.automation.lastSyncedAt" />
  <ProviderCardPricingNote :provider="provider" />
  <!-- 自动刷新被判失败后进入冷却，写清楚何时恢复，别让人以为卡住不动。 -->
  <ProviderCardCooldownNote :provider="provider" />
  <!-- 主地址不可用、请求改走备用地址时，卡片说明当前实际使用的是哪个地址。 -->
  <ProviderCardEndpointNote :provider="provider" />
</section>
</div>

<!-- 模型行是正文最后一层，多余的高度留在额度和模型之间，芯片紧贴页脚
     分界线；有测活时间线时由时间线收口。行数固定三行，不由卡型决定。 -->
<ProviderModelPreview
  :models="provider.visibleModels"
  :state="provider.capabilities.availableModelsState"
  :sync-time="modelSyncTime"
  :interactive="interactive"
  :show-overflow="!list"
  :class="{ 'provider-card-models-tail': !showLivenessTimeline }"
  @open="emit('models')"
/>

<ProviderLivenessTimeline
  v-if="showLivenessTimeline"
  :records="livenessRecords"
  :empty-label="livenessEmptyLabel"
/>
</div>
</template>
