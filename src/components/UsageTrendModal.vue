<script setup lang="ts">
import { computed } from "vue";
import { IconRefresh } from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderUsageSummary } from "../stores/providers";
import { useUsageTrendChart } from "../composables/useUsageTrendChart";
import { formatNumberCompact, formatQuotaValue } from "../utils/provider-display";
import {
  usagePeriodOptions,
  type UsagePeriod,
} from "../utils/usage-trend";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  summary: ProviderUsageSummary | null;
  period: UsagePeriod;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:period": [period: UsagePeriod];
  refresh: [];
}>();

const {
  maxUsageValue,
  title: usagePeriodTitle,
  usageAreaPath,
  usageAverage,
  usageChartPoints,
  usageLinePath,
  usagePeakPoint,
  usageRequestTotal,
  usageTokenTotal,
  usageTotal,
  usageXAxisLabels,
  usageYAxisLabels,
} = useUsageTrendChart(props);

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · ${usagePeriodTitle.value}` : "用量趋势",
);

const periodModel = computed({
  get: () => props.period,
  set: (value: UsagePeriod) => emit("update:period", value),
});
</script>

<template>
  <a-modal
    :visible="visible"
    :title="modalTitle"
    :footer="false"
    :width="760"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="usage-panel">
      <div class="usage-toolbar">
        <a-radio-group v-model="periodModel" type="button" :options="usagePeriodOptions" />
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新趋势
        </a-button>
      </div>
      <a-spin :loading="loading">
        <div v-if="!summary || summary.points.length === 0" class="api-key-empty">
          暂无用量数据
        </div>
        <div v-else class="usage-dashboard">
          <div class="usage-summary-cards">
            <div>
              <span>总消耗</span>
              <strong>{{ formatQuotaValue(usageTotal, summary.quotaDisplay) }}</strong>
            </div>
            <div>
              <span>峰值</span>
              <strong>{{ formatQuotaValue(maxUsageValue, summary.quotaDisplay) }}</strong>
            </div>
            <div>
              <span>平均</span>
              <strong>{{ formatQuotaValue(usageAverage, summary.quotaDisplay) }}</strong>
            </div>
            <div>
              <span>请求数</span>
              <strong>{{ formatNumberCompact(usageRequestTotal, 0) }}</strong>
            </div>
            <div>
              <span>Token</span>
              <strong>{{ formatNumberCompact(usageTokenTotal, 0) }}</strong>
            </div>
          </div>
          <div class="usage-chart">
            <div class="usage-chart-header">
              <div>
                <strong>额度消耗趋势</strong>
                <span>{{ usagePeriodTitle }}</span>
              </div>
              <div class="usage-chart-legend">
                <i />
                <span>消耗额度</span>
              </div>
            </div>
            <svg viewBox="0 0 560 220" role="img" aria-label="用量趋势图">
              <defs>
                <linearGradient id="usageAreaGradient" x1="0" x2="0" y1="0" y2="1">
                  <stop offset="0%" stop-color="#f53f3f" stop-opacity="0.32" />
                  <stop offset="100%" stop-color="#f53f3f" stop-opacity="0.04" />
                </linearGradient>
              </defs>
              <g class="usage-grid">
                <g v-for="label in usageYAxisLabels" :key="`y-${label.y}`">
                  <line x1="54" x2="542" :y1="label.y" :y2="label.y" />
                  <text x="46" :y="label.y + 4">{{ formatQuotaValue(label.value, summary.quotaDisplay) }}</text>
                </g>
              </g>
              <path class="usage-area" :d="usageAreaPath" />
              <path class="usage-line" :d="usageLinePath" />
              <g v-for="point in usageChartPoints" :key="`${point.date}-dot`">
                <line class="usage-column" :x1="point.x" :x2="point.x" y1="182" :y2="point.y" />
                <circle class="usage-dot" :cx="point.x" :cy="point.y" r="3.4">
                  <title>{{ point.date }} · {{ formatQuotaValue(point.used, summary.quotaDisplay) }}</title>
                </circle>
              </g>
              <g v-if="usagePeakPoint" class="usage-peak">
                <circle :cx="usagePeakPoint.x" :cy="usagePeakPoint.y" r="5" />
                <text
                  :x="Math.min(usagePeakPoint.x + 8, 464)"
                  :y="Math.max(usagePeakPoint.y - 8, 14)"
                >
                  峰值 {{ formatQuotaValue(usagePeakPoint.used, summary.quotaDisplay) }}
                </text>
              </g>
              <g class="usage-axis-labels">
                <text
                  v-for="point in usageXAxisLabels"
                  :key="`x-${point.date}`"
                  :x="point.x"
                  y="210"
                >
                  {{ point.date }}
                </text>
              </g>
            </svg>
            <div class="usage-list">
              <div v-for="point in summary.points.slice(-8).reverse()" :key="point.date" class="usage-row">
                <span class="usage-date">{{ point.date }}</span>
                <div class="usage-bar-track">
                  <div class="usage-bar" :style="{ width: `${Math.max(4, (point.used / maxUsageValue) * 100)}%` }" />
                </div>
                <strong>{{ formatQuotaValue(point.used, summary.quotaDisplay) }}</strong>
                <span class="usage-meta">{{ point.requestCount }} 次 · {{ formatNumberCompact(point.tokenUsed, 0) }} tokens</span>
              </div>
            </div>
          </div>
        </div>
      </a-spin>
    </div>
  </a-modal>
</template>
