<script setup lang="ts">
import { computed } from "vue";
import {
  IconBarChart,
  IconFire,
  IconLayers,
  IconRefresh,
  IconThunderbolt,
} from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderUsageModelStat, ProviderUsageSummary } from "../stores/providers";
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
  usageAverageRpm,
  usageAverageTpm,
  usageChartPoints,
  usageLinePath,
  usageMaxModelCalls,
  usageMaxModelQuota,
  usageModelStats,
  usagePeakPoint,
  usageRequestTotal,
  usageTimeRangeMinutes,
  usageTokenTotal,
  usageTopCallModels,
  usageTopQuotaModels,
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

const usageStatCards = computed(() => [
  {
    key: "count",
    title: "总请求",
    value: formatNumberCompact(usageRequestTotal.value, 0),
    icon: IconBarChart,
  },
  {
    key: "quota",
    title: "总消耗",
    value: props.summary ? formatQuotaValue(usageTotal.value, props.summary.quotaDisplay) : "-",
    icon: IconFire,
  },
  {
    key: "tokens",
    title: "总 Tokens",
    value: formatNumberCompact(usageTokenTotal.value, 0),
    icon: IconLayers,
  },
  {
    key: "rpm",
    title: "平均 RPM",
    value: formatNumberCompact(usageAverageRpm.value, 3),
    icon: IconThunderbolt,
  },
  {
    key: "tpm",
    title: "平均 TPM",
    value: formatNumberCompact(usageAverageTpm.value, 3),
    icon: IconThunderbolt,
  },
]);

function modelName(item: ProviderUsageModelStat) {
  return item.modelName.trim() || "未知模型";
}

function modelQuotaPercent(item: ProviderUsageModelStat) {
  return barPercent(item.used, usageMaxModelQuota.value);
}

function modelCallPercent(item: ProviderUsageModelStat) {
  return barPercent(item.requestCount, usageMaxModelCalls.value);
}

function barPercent(value: number, max: number) {
  if (!Number.isFinite(value) || value <= 0) return "0%";
  return `${Math.max(4, Math.min(100, (value / Math.max(1, max)) * 100))}%`;
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal usage-trend-modal"
    :footer="false"
    :width="980"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title usage-trend-title">
        <span class="surface-modal-title-icon"><icon-bar-chart /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>
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
            <div v-for="card in usageStatCards" :key="card.key" class="usage-summary-card">
              <span>
                <component :is="card.icon" />
                {{ card.title }}
              </span>
              <strong>{{ card.value }}</strong>
            </div>
          </div>
          <div class="usage-chart">
            <div class="usage-chart-header">
              <div>
                <strong>额度消耗趋势</strong>
                <span>{{ usagePeriodTitle }} · {{ usageTimeRangeMinutes }} 分钟</span>
              </div>
              <div class="usage-chart-legend">
                <i />
                <span>总消耗 {{ formatQuotaValue(usageTotal, summary.quotaDisplay) }}</span>
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
          <div v-if="usageModelStats.length > 0" class="usage-model-dashboard">
            <section class="usage-model-panel">
              <div class="usage-model-panel-header">
                <strong>模型额度排行</strong>
                <span>按消耗额度排序</span>
              </div>
              <div class="usage-model-list">
                <div v-for="item in usageTopQuotaModels" :key="`quota-${modelName(item)}`" class="usage-model-row">
                  <div class="usage-model-row-head">
                    <strong :title="modelName(item)">{{ modelName(item) }}</strong>
                    <span>{{ formatQuotaValue(item.used, summary.quotaDisplay) }}</span>
                  </div>
                  <div class="usage-model-bar-track">
                    <div class="usage-model-bar usage-model-bar-quota" :style="{ width: modelQuotaPercent(item) }" />
                  </div>
                  <small>{{ formatNumberCompact(item.requestCount, 0) }} 次 · {{ formatNumberCompact(item.tokenUsed, 0) }} tokens</small>
                </div>
              </div>
            </section>
            <section class="usage-model-panel">
              <div class="usage-model-panel-header">
                <strong>模型调用分析</strong>
                <span>按请求次数排序</span>
              </div>
              <div class="usage-model-list">
                <div v-for="item in usageTopCallModels" :key="`call-${modelName(item)}`" class="usage-model-row">
                  <div class="usage-model-row-head">
                    <strong :title="modelName(item)">{{ modelName(item) }}</strong>
                    <span>{{ formatNumberCompact(item.requestCount, 0) }} 次</span>
                  </div>
                  <div class="usage-model-bar-track">
                    <div class="usage-model-bar usage-model-bar-call" :style="{ width: modelCallPercent(item) }" />
                  </div>
                  <small>{{ formatQuotaValue(item.used, summary.quotaDisplay) }} · {{ formatNumberCompact(item.tokenUsed, 0) }} tokens</small>
                </div>
              </div>
            </section>
          </div>
        </div>
      </a-spin>
    </div>
  </a-modal>
</template>
