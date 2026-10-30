<script setup lang="ts">
import { computed } from "vue";
import { ArrowUpRight } from "@lucide/vue";
import type { Provider } from "../../stores/provider-types";
import AccountQuotaBar from "../oauth/AccountQuotaBar.vue";
import ProviderCardStaleNote from "./ProviderCardStaleNote.vue";
import ProviderModelPreview from "../ProviderModelPreview.vue";
import {
  managedQuotaWindowRows,
  formatProviderSyncTime,
  providerQuotaBarWindow,
} from "../../utils/provider-display";
import {
  quotaNumber,
  quotaPercent,
  quotaWindowPercent,
} from "../../utils/oauth-display";
const props = defineProps<{ provider: Provider; interactive: boolean }>();
const emit = defineEmits<{ details: []; models: [] }>();
const subscription = computed(() => props.provider.capabilities.subscription);
const gateway = computed(() => props.provider.capabilities.gateway);
/// Rust stores the windows tightest first, so the card shows the two that
/// decide whether the account still has room instead of the provider's order.
/// 和中转站卡片同一套窗口条读法：窗口名、重置倒计时、已用比例一行，条在下。
const windowRows = computed(() =>
  managedQuotaWindowRows(subscription.value?.windows || []),
);
const hiddenWindows = computed(
  () => (subscription.value?.windows || []).slice(3),
);
const hiddenSummary = computed(() => {
  const [first] = hiddenWindows.value;
  if (!first) return "";
  const percent = quotaWindowPercent(first);
  return percent == null
    ? first.label
    : `${first.label} ${quotaPercent(percent)}%`;
});
const hiddenTitle = computed(() =>
  hiddenWindows.value
    .map((window) => {
      const percent = quotaWindowPercent(window);
      return percent == null
        ? window.label
        : `${window.label} ${quotaPercent(percent)}%`;
    })
    .join(" · "),
);
/// 模型区和中转站卡片共用同一块 ProviderModelPreview：标题、计数、三行芯片和
/// 空态都由它给出，账号卡和网关卡不再自己画一套。
const modelSyncTime = computed(() =>
  formatProviderSyncTime(props.provider.capabilities.availableModelsState.updatedAt),
);
const enabled = computed(
  () => gateway.value?.accounts.filter((a) => a.available).length || 0,
);
const requestTotal = computed(() => gateway.value?.totalRequests ?? 0);
const requestFailed = computed(() => gateway.value?.failedRequests ?? 0);
const successPercent = computed(() =>
  requestTotal.value > 0
    ? Math.round(((requestTotal.value - requestFailed.value) / requestTotal.value) * 1000) / 10
    : null,
);
/// 钱包余额在中转站卡片上紧跟在额度标题旁；账号卡把它放在窗口条下面一行，
/// 和说明文字同一段，不另占一行容量。
const creditsLabel = computed(() => {
  if (subscription.value?.creditsUnlimited) return "余额不限额";
  if (subscription.value?.credits != null) {
    return `余额 ${quotaNumber(subscription.value.credits)} ${subscription.value.creditsUnit || ""}`.trim();
  }
  return "";
});
</script>
<template>
  <div class="managed-card-body">
    <template v-if="provider.actions.oauthAccount">
      <!-- 卡片正文不预留页脚空白：窗口条按内容排布，模型芯片是正文最后一层，
           紧贴页脚分界线。 -->
      <div v-if="windowRows.length" class="managed-card-quotas">
        <AccountQuotaBar
          v-for="row in windowRows"
          :key="row.label"
          :window="providerQuotaBarWindow(row)"
          compact
        />
      </div>
      <div v-else class="managed-card-empty">
        {{ subscription?.error || "同步后查看账号额度" }}
      </div>
      <!-- 钱包余额是溢出账户，读法和中转站卡片的「本周剩余 $x」一样放在额度
           区块里，不和窗口条抢同一行的说明位置。 -->
      <div v-if="creditsLabel" class="managed-card-credits">
        {{ creditsLabel }}
      </div>
      <!-- 没画出来的窗口是额度读数的一部分，紧跟额度区块；续期异常由底栏的
           状态说明和「重新授权」按钮负责，正文不再重复同一句话。 -->
      <div
        v-if="hiddenWindows.length || provider.auth.oauth?.organizationName"
        class="managed-card-notes"
      >
        <button
          v-if="hiddenWindows.length"
          type="button"
          :disabled="!interactive"
          :title="hiddenTitle"
          @click.stop="emit('details')"
          @pointerdown.stop
        >
          还有 {{ hiddenWindows.length }} 项 · {{ hiddenSummary }}
          <ArrowUpRight :size="12" />
        </button>
        <span v-else-if="provider.auth.oauth?.organizationName">{{
          provider.auth.oauth.organizationName
        }}</span>
      </div>
      <!-- 数据新鲜时卡片不写同步时间；过期了才说一句，避免拿旧数字做判断。 -->
      <ProviderCardStaleNote :synced-at="provider.automation.lastSyncedAt" />
      <ProviderModelPreview
        class="provider-card-models-tail"
        :models="provider.capabilities.availableModels"
        :state="provider.capabilities.availableModelsState"
        :sync-time="modelSyncTime"
        :interactive="interactive"
        @open="emit('models')"
      />
    </template>
    <template v-else>
      <!-- 网关卡只换数据源，不换排版：主数字和成功率条沿用中转站卡片的额度
           区块（同一组 class、同一支进度条），逐时间桶的请求分布属于明细，
           留在网关面板里。 -->
      <section
        v-if="gateway"
        class="provider-card-quota"
        aria-label="网关授权与请求"
      >
        <div class="provider-card-balance">
          <span class="provider-card-balance-labels"><span>可用授权</span></span>
          <strong :title="`可用 ${enabled} 个，共 ${gateway.accounts.length} 个`"
            >{{ enabled }}<small>/ {{ gateway.accounts.length }}</small></strong
          >
        </div>
        <div
          v-if="successPercent != null"
          class="provider-card-progress-row"
          :title="`成功 ${quotaNumber(requestTotal - requestFailed)} · 失败 ${quotaNumber(requestFailed)} · 共 ${quotaNumber(requestTotal)} 次请求`"
        >
          <span class="provider-card-quota-meter"
            >{{ successPercent }}%<small>成功率</small></span
          >
          <a-progress
            class="provider-quota-progress"
            :percent="successPercent"
            :show-text="false"
            size="small"
          />
        </div>
      </section>
      <p v-else class="managed-gateway-empty">
        连接后查看网关账号
      </p>
      <!-- 数据新鲜时卡片不写同步时间；过期了才说一句，避免拿旧数字做判断。 -->
      <ProviderCardStaleNote :synced-at="provider.automation.lastSyncedAt" />
      <!-- 网关卡和中转站卡、账号卡共用同一块模型区：不再只报一个「可用模型 N」
           的数字，也不自己画一套芯片。 -->
      <ProviderModelPreview
        class="provider-card-models-tail"
        :models="provider.capabilities.availableModels"
        :state="provider.capabilities.availableModelsState"
        :sync-time="modelSyncTime"
        :interactive="interactive"
        @open="emit('models')"
      />
    </template>
  </div>
</template>
<style scoped>
.managed-card-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 11px;
  padding: 11px 0 2px;
}
.managed-card-quotas {
  display: grid;
  gap: 12px;
}
.managed-card-empty {
  flex: 1;
  display: flex;
  align-items: center;
  font-size: 12px;
  line-height: 1.6;
  color: var(--color-text-3);
  overflow: hidden;
}
.managed-card-notes {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--color-text-3);
  font-size: 11px;
}
.managed-card-notes > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 钱包余额紧贴窗口条，和主数字同一段。 */
.managed-card-credits {
  color: var(--color-text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  line-height: 1.35;
}
.managed-card-notes button {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  max-width: 100%;
  overflow: hidden;
  border: 0;
  border-radius: 6px;
  background: none;
  padding: 3px 0;
  color: var(--managed-accent-ink, var(--color-text-2));
  font-size: inherit;
  font-weight: 500;
  cursor: pointer;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.managed-card-notes button:hover {
  color: rgb(var(--primary-6));
}
.managed-card-notes button:disabled {
  cursor: default;
  opacity: 0.5;
}
/* 网关还没连上时正文只有这一句：模型和成功率条都要等网关快照。 */
.managed-gateway-empty {
  margin: 0;
  color: var(--color-text-3);
  font-size: 12px;
  line-height: 1.6;
}
</style>
