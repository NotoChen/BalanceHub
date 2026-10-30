<script setup lang="ts">
import { computed } from "vue";
import type { AccountQuotaWindow } from "../../stores/oauth-types";
import {
  quotaNumber,
  quotaPercent,
  quotaResetLabel,
  quotaTrendSummary,
  quotaWindowPercent,
  quotaWindowUnlimited,
} from "../../utils/oauth-display";
import { quotaLevel } from "../../utils/quota-level";
import QuotaSparkline from "./QuotaSparkline.vue";
const props = defineProps<{
  window: AccountQuotaWindow;
  compact?: boolean;
  now?: number;
}>();
const percent = computed(() => quotaWindowPercent(props.window));
/// 没有比例可画的窗口只有一种解释：这条额度本来就没有上限。窗口自己带着
/// 上限和使用量，读法不该由每张卡片各传一次。
const unlimited = computed(() => quotaWindowUnlimited(props.window));
const level = computed(() => quotaLevel(percent.value));
const remaining = computed(() =>
  percent.value == null ? null : Math.max(0, 100 - percent.value),
);
const trendTitle = computed(() => quotaTrendSummary(props.window.history));
/// The allowance caption is a full sentence in the tooltip; the row itself only
/// keeps the reset countdown, because a fixed-height card cannot spare a second
/// caption line under every window.
const rangeTitle = computed(() =>
  props.window.used != null || props.window.limit != null
    ? `${quotaNumber(props.window.used)} / ${quotaNumber(props.window.limit)} ${props.window.unit ?? ""}`.trim()
    : "",
);
const metaLabel = computed(() => {
  if (props.window.resetAt) return quotaResetLabel(props.window.resetAt, props.now);
  return "";
});
</script>

<template>
  <div
    class="account-quota"
    :class="{
      'account-quota-compact': compact,
      [`account-quota-${level}`]: true,
    }"
  >
    <div class="account-quota-line">
      <span class="account-quota-label" :title="window.label">{{
        window.label
      }}</span>
      <span class="account-quota-caption">
        <span v-if="trendTitle" class="account-quota-trend" :title="trendTitle">
          <QuotaSparkline :samples="window.history" />
        </span>
        <span v-if="metaLabel" class="account-quota-meta-note" :title="rangeTitle">{{
          metaLabel
        }}</span>
      </span>
      <span
        class="account-quota-value"
        :title="
          remaining == null
            ? unlimited
              ? '该窗口不限量'
              : '站点未提供已用比例'
            : `剩余 ${quotaPercent(remaining)}%`
        "
        >{{
          percent == null
            ? unlimited
              ? "不限"
              : "未提供比例"
            : `${quotaPercent(percent)}%`
        }}<small v-if="percent != null">已用</small></span
      >
    </div>
    <div
      v-if="percent != null"
      class="account-quota-track"
      role="progressbar"
      :aria-label="`${window.label}已用`"
      :aria-valuenow="Math.min(100, Math.max(0, percent))"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuetext="`${quotaPercent(percent)}% 已用`"
    >
      <span
        :style="{
          transform: `scaleX(${Math.min(100, Math.max(0, percent)) / 100})`,
        }"
      />
    </div>
    <div v-else class="account-quota-track account-quota-track-unknown" />
  </div>
</template>

<style scoped>
.account-quota {
  --quota-fill: var(--managed-accent, #165dff);
  --quota-fill-soft: color-mix(in srgb, var(--quota-fill) 62%, #ffffff);
  min-width: 0;
  display: grid;
  gap: 6px;
}
.account-quota-warn {
  --quota-fill: #d98a00;
  --quota-fill-soft: #f7bb3c;
}
.account-quota-high {
  --quota-fill: #e34d59;
  --quota-fill-soft: #f08a92;
}
/* 重置时间是次要信息：宽度不够时先让名字和比例读全，说明文字缩掉。 */
.account-quota-meta-note {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.account-quota-trend {
  display: inline-flex;
  align-items: center;
  color: var(--quota-fill);
}
.account-quota-compact .account-quota-trend :deep(.quota-spark) {
  width: 34px;
  height: 12px;
}
.account-quota-line {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.account-quota-label {
  flex: 1 1 auto;
  min-width: 3.5em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--color-text-2);
  font-size: 12px;
  line-height: 16px;
}
.account-quota-value {
  flex-shrink: 0;
  color: var(--color-text-1);
  font-size: 13px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
  line-height: 16px;
}
.account-quota-value small {
  margin-left: 4px;
  color: var(--color-text-3);
  font-size: 10px;
  font-weight: 400;
  letter-spacing: 0;
}
.account-quota-high .account-quota-value {
  color: #cb2634;
}
.account-quota-track {
  position: relative;
  height: 6px;
  border-radius: 999px;
  overflow: hidden;
  background: color-mix(in srgb, var(--color-fill-3) 76%, transparent);
  box-shadow: inset 0 1px 1px rgba(29, 33, 41, 0.05);
}
.account-quota-track span {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(
    90deg,
    var(--quota-fill) 0%,
    var(--quota-fill-soft) 100%
  );
  transform-origin: left;
  transition: transform 0.32s ease;
}
/* 站点没报比例时留一条空槽：高度和真实进度条一致，同一排几个窗口的
   名称才能对齐。 */
.account-quota-track-unknown {
  opacity: 0.6;
}
/* 窗口名和重置时间同排：固定高度的卡片每多一行说明，模型行就少一行。 */
.account-quota-caption {
  display: inline-flex;
  min-width: 0;
  align-items: baseline;
  gap: 6px;
  flex: 0 1 auto;
  color: var(--color-text-3);
  font-size: 10px;
  line-height: 13px;
  font-variant-numeric: tabular-nums;
}
.account-quota-compact {
  gap: 4px;
}
@media (prefers-reduced-motion: reduce) {
  .account-quota-track span {
    transition: none;
  }
}
</style>
