<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Gauge } from "@lucide/vue";
import type { Provider } from "../../stores/provider-types";
import type {
  CpaAccountSummary,
  CpaAccountQuota,
  CpaQuotaBucket,
} from "../../stores/oauth-types";
import { getCpaAccountQuota } from "../../api/oauth-accounts";
import { quotaNumber, quotaPercent, quotaResetLabel } from "../../utils/oauth-display";

const props = defineProps<{
  provider: Provider;
  account: CpaAccountSummary;
  apiVersion: string;
}>();

/// A quota reading makes the gateway ask the provider, so it is read only when
/// the row is expanded and the user asks for it rather than on every refresh.
const quota = ref<CpaAccountQuota | null>(null);
const loading = ref(false);
const error = ref("");
let request = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  request++;
});
watch(
  () => [props.provider.identity.id, props.account.name],
  () => {
    request++;
    quota.value = null;
    error.value = "";
    loading.value = false;
  },
);

const plan = computed(() => quota.value?.plan || quota.value?.tier || "");
async function load() {
  if (loading.value) return;
  const sequence = ++request;
  loading.value = true;
  error.value = "";
  try {
    const result = await getCpaAccountQuota(
      props.provider.identity.id,
      props.account.authIndex,
      props.account.quotaPluginId,
      props.apiVersion,
    );
    if (disposed || sequence !== request) return;
    quota.value = result;
  } catch (cause) {
    if (!disposed && sequence === request) error.value = String(cause);
  } finally {
    if (sequence === request) loading.value = false;
  }
}
/// `remainingFraction` is what the provider has left, so the meter draws usage
/// from its complement. A provider that reports no fraction leaves the meter
/// empty instead of claiming a full window.
function usedPercent(bucket: CpaQuotaBucket) {
  const left = bucket.remainingFraction;
  return left == null || !Number.isFinite(left)
    ? null
    : Math.min(100, Math.max(0, (1 - left) * 100));
}
function remainingText(bucket: CpaQuotaBucket) {
  return bucket.remainingFraction == null
    ? ""
    : `剩 ${quotaPercent(bucket.remainingFraction * 100)}%`;
}
function bucketLabel(bucket: CpaQuotaBucket) {
  return bucket.window || bucket.description || "额度";
}
function metricValue(value: number, unit: string, format: string, currency: string) {
  if (format === "currency" && currency) {
    try {
      return new Intl.NumberFormat("zh-CN", {
        style: "currency",
        currency,
      }).format(value);
    } catch {
      return quotaNumber(value);
    }
  }
  return unit ? `${quotaNumber(value)} ${unit}` : quotaNumber(value);
}
</script>
<template>
  <div class="cpa-quota">
    <div class="cpa-quota-head">
      <span>额度</span>
      <span v-if="plan" class="cpa-quota-plan">{{ plan }}</span>
      <a-button size="mini" :loading="loading" @click="load">{{
        quota ? "重新读取" : "读取额度"
      }}</a-button>
    </div>
    <p v-if="error" class="cpa-quota-error">{{ error }}</p>
    <template v-else-if="quota">
      <div v-if="quota.metrics.length" class="cpa-quota-metrics">
        <div v-for="metric in quota.metrics" :key="metric.key">
          <strong>{{
            metricValue(metric.value, metric.unit, metric.format, metric.currency)
          }}</strong
          ><span>{{ metric.label }}</span>
        </div>
      </div>
      <div v-for="group in quota.groups" :key="group.displayName" class="cpa-quota-group">
        <span v-if="group.displayName" class="cpa-quota-group-name">{{
          group.displayName
        }}</span>
        <div
          v-for="bucket in group.buckets"
          :key="`${bucket.window}-${bucket.resetAt}`"
          class="cpa-quota-bucket"
        >
          <div class="cpa-quota-bucket-copy">
            <span :title="bucket.description">{{ bucketLabel(bucket) }}</span>
            <span
              >{{ remainingText(bucket)
              }}<template v-if="bucket.resetAt">
                · {{ quotaResetLabel(bucket.resetAt) }}</template
              ></span
            >
          </div>
          <div
            class="cpa-quota-bar"
            role="progressbar"
            :aria-label="`${bucketLabel(bucket)} 已用`"
            aria-valuemin="0"
            aria-valuemax="100"
            :aria-valuenow="usedPercent(bucket) ?? undefined"
          >
            <span
              :class="{ unknown: usedPercent(bucket) == null }"
              :style="{ width: `${usedPercent(bucket) ?? 0}%` }"
            />
          </div>
        </div>
      </div>
      <p v-if="!quota.metrics.length && !quota.groups.length" class="cpa-quota-empty">
        <Gauge :size="13" />网关返回了额度结果，但没有可展示的窗口或汇总。
      </p>
    </template>
    <p v-else class="cpa-quota-hint">
      额度由网关上的插件读取，会向上游查询一次。
    </p>
  </div>
</template>
<style scoped>
.cpa-quota {
  display: grid;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--color-border-2);
  border-radius: 9px;
  background: var(--color-fill-1);
}
.cpa-quota-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.cpa-quota-head > span:first-child {
  color: var(--color-text-2);
  font-size: 11px;
}
.cpa-quota-plan {
  flex: 1;
  overflow: hidden;
  color: var(--color-text-1);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-quota-head :deep(.arco-btn) {
  margin-left: auto;
}
.cpa-quota-hint,
.cpa-quota-empty {
  display: flex;
  align-items: center;
  gap: 5px;
  margin: 0;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.6;
}
.cpa-quota-error {
  margin: 0;
  color: rgb(var(--orange-6));
  font-size: 11px;
  line-height: 1.6;
}
.cpa-quota-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
}
.cpa-quota-metrics > div {
  display: grid;
  gap: 1px;
}
.cpa-quota-metrics strong {
  color: var(--color-text-1);
  font-size: 13px;
  font-weight: 600;
}
.cpa-quota-metrics span {
  color: var(--color-text-3);
  font-size: 10px;
}
.cpa-quota-group {
  display: grid;
  gap: 6px;
}
.cpa-quota-group-name {
  color: var(--color-text-2);
  font-size: 11px;
}
.cpa-quota-bucket {
  display: grid;
  gap: 4px;
}
.cpa-quota-bucket-copy {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  font-size: 11px;
}
.cpa-quota-bucket-copy > span:first-child {
  overflow: hidden;
  color: var(--color-text-2);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-quota-bucket-copy > span:last-child {
  flex-shrink: 0;
  color: var(--color-text-3);
}
.cpa-quota-bar {
  height: 5px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--color-fill-3);
}
.cpa-quota-bar > span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: rgb(var(--primary-6));
}
.cpa-quota-bar > span.unknown {
  width: 100% !important;
  background: var(--color-fill-4);
}
</style>
