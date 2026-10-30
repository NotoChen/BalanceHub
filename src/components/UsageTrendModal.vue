<script setup lang="ts">
import { computed } from "vue";
import {
  IconBarChart,
  IconFire,
  IconLayers,
  IconRefresh,
  IconThunderbolt,
} from "@arco-design/web-vue/es/icon";
import type {
  Provider,
  ProviderUsageModelStat,
  ProviderUsageOverview,
  ProviderUsageSummary,
} from "../stores/providers";
import { useUsageTrendChart } from "../composables/useUsageTrendChart";
import { formatNumberCompact, formatQuotaValue, providerDisplayLabel, providerQuotaWindowRows } from "../utils/provider-display";
import { quotaResetLabel } from "../utils/oauth-display";
import {
  usagePeriodOptions,
  type UsagePeriod,
} from "../utils/usage-trend";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  error: string;
  summary: ProviderUsageSummary | null;
  /// 不带中转站时由顶栏打开，展示全部站点的汇总。
  overview?: ProviderUsageOverview | null;
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
  usageModelHealth,
  usagePeakPoint,
  usagePeriodGranularity,
  usageRequestTotal,
  usageTokenTotal,
  usageTopCallModels,
  usageTopQuotaModels,
  usageTotal,
  usageXAxisLabels,
  usageYAxisLabels,
} = useUsageTrendChart(props);

const modalTitle = computed(() =>
  props.provider
    ? `${providerDisplayLabel(props.provider)} · ${usagePeriodTitle.value}`
    : `全部中转站 · ${usagePeriodTitle.value}`,
);

const periodModel = computed({
  get: () => props.period,
  set: (value: UsagePeriod) => emit("update:period", value),
});

/// 订阅制中转站的额度在周期窗口里，趋势面板说清最近一次额度重置，
/// 读者才不会把“本月已用”当成钱包扣费。
const subscriptionNote = computed(() => {
  const provider = props.provider;
  if (!provider) return null;
  const rows = providerQuotaWindowRows(provider);
  if (rows.length === 0) return null;
  const reset = (provider.quota.windows ?? [])
    .map((window) => window.resetsAt)
    .find((value) => value && value * 1000 > Date.now());
  return {
    label: rows.length > 1 ? `${rows.length} 个订阅窗口` : "订阅额度",
    detail: reset ? quotaResetLabel(reset) : "",
  };
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
    value: props.summary ? formatQuotaValue(usageTotal.value, props.summary.quotaDisplay, 6) : "—",
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
    value: formatRate(usageAverageRpm.value),
    icon: IconThunderbolt,
    hint: "所选时段内，平均每分钟的请求次数",
  },
  {
    key: "tpm",
    title: "平均 TPM",
    value: formatRate(usageAverageTpm.value),
    icon: IconThunderbolt,
    hint: "所选时段内，平均每分钟消耗的 Tokens",
  },
]);

/// 汇总视图没有单一配额单位，能说的是次数、模型排行和响应质量；
/// 站点读数失败时照常展示其余站点，并把失败站点列在底部。
const overviewStats = computed(() => {
  const overview = props.overview;
  const minutes = props.period === "24h" ? 24 * 60 : props.period === "7d" ? 7 * 24 * 60 : 30 * 24 * 60;
  const modelStats = overview?.modelStats ?? [];
  return {
    requests: overview?.requestCount ?? 0,
    models: modelStats.length,
    rpm: (overview?.requestCount ?? 0) / minutes,
    modelStats,
    /// 排行条以汇总列表里最大的次数为满格，和单站的模型排行同一口径。
    maxCalls: Math.max(1, ...modelStats.map((item) => item.requestCount)),
    modelHealth: overview?.modelHealth ?? [],
    errors: overview?.errors ?? [],
  };
});

function overviewCallPercent(item: ProviderUsageModelStat) {
  return barPercent(item.requestCount, overviewStats.value.maxCalls);
}

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
  return `${Math.max(4, Math.min(100, (value / (max > 0 ? max : 1)) * 100))}%`;
}

function formatPercent(value: number) {
  return `${value.toFixed(value >= 99 ? 1 : 0)}%`;
}

/// 失败率是真正要看的数字，所以按成功率给颜色，而不是按耗时。
function healthTone(successRate: number) {
  if (successRate >= 99) return "usage-health-ok";
  if (successRate >= 95) return "usage-health-warning";
  return "usage-health-error";
}

function formatRate(value: number) {
  return formatNumberCompact(value, value > 0 && value < 0.001 ? 6 : 3);
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
          <small v-if="!provider">全部中转站 · 次数与模型口径，不含配额单位</small>
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
      <div v-if="subscriptionNote" class="usage-subscription-note">
        <span>订阅额度 · {{ subscriptionNote.label }}</span>
        <span v-if="subscriptionNote.detail">{{ subscriptionNote.detail }}</span>
      </div>
      <a-alert v-if="error" type="error" show-icon>
        <div class="panel-load-error"><span>{{ error }}{{ summary ? '；当前保留上次读取的结果' : '' }}</span><a-button size="small" @click="emit('refresh')">重试</a-button></div>
      </a-alert>
      <a-spin v-if="!provider" :loading="loading">
        <div v-if="!overview || (error && overview.requestCount === 0 && overview.errors.length === 0)" class="api-key-empty" role="status">
          {{ loading ? '正在读取各站点的用量…' : error ? '用量数据未能加载' : '暂无用量数据' }}
        </div>
        <div v-else class="usage-dashboard">
          <div class="usage-summary-cards is-overview">
            <div class="usage-summary-card" :title="`${usagePeriodTitle}内全部中转站的请求合计`">
              <!-- 读取失败时上方已经有错误提示，这里只保留可读的数字。 -->
              <span><IconBarChart /> 总请求</span>
              <strong>{{ formatNumberCompact(overviewStats.requests, 0) }}</strong>
            </div>
            <div class="usage-summary-card" title="参与汇总的模型数量">
              <span><IconLayers /> 涉及模型</span>
              <strong>{{ overviewStats.models }}</strong>
            </div>
            <div class="usage-summary-card" title="所选时段内，平均每分钟的请求次数">
              <span><IconThunderbolt /> 平均 RPM</span>
              <strong>{{ formatRate(overviewStats.rpm) }}</strong>
            </div>
            <div class="usage-summary-card" title="读取失败的站点">
              <span><IconFire /> 未读取站点</span>
              <strong>{{ overviewStats.errors.length }}</strong>
            </div>
          </div>
          <div v-if="overviewStats.modelStats.length || overviewStats.modelHealth.length" class="usage-model-dashboard">
            <section v-if="overviewStats.modelStats.length" class="usage-model-panel">
              <div class="usage-model-panel-header">
                <strong>模型调用排行</strong>
                <span>全部中转站合计 · 前 {{ overviewStats.modelStats.length }} 个</span>
              </div>
              <div class="usage-model-list">
                <div v-for="item in overviewStats.modelStats" :key="`overview-${modelName(item)}`" class="usage-model-row">
                  <div class="usage-model-row-head">
                    <strong :title="modelName(item)">{{ modelName(item) }}</strong>
                    <span>{{ formatNumberCompact(item.requestCount, 0) }} 次</span>
                  </div>
                  <div class="usage-model-bar-track">
                    <div class="usage-model-bar usage-model-bar-call" :style="{ width: overviewCallPercent(item) }" />
                  </div>
                  <small>{{ formatNumberCompact(item.tokenUsed, 0) }} tokens</small>
                </div>
              </div>
            </section>
            <section v-if="overviewStats.modelHealth.length" class="usage-model-panel usage-health-panel">
              <div class="usage-model-panel-header">
                <strong>响应质量</strong>
                <span>站点近 24 小时采样 · 成功率低的在前</span>
              </div>
              <div class="usage-model-list">
                <div v-for="item in overviewStats.modelHealth" :key="`overview-health-${item.modelName}`" class="usage-model-row">
                  <div class="usage-model-row-head">
                    <strong :title="item.modelName">{{ item.modelName }}</strong>
                    <span v-if="item.successRate !== null" :class="healthTone(item.successRate)">
                      成功率 {{ formatPercent(item.successRate) }}
                    </span>
                    <span v-else>成功率未采样</span>
                  </div>
                  <small>
                    <template v-if="item.avgLatencyMs !== null">平均耗时 {{ item.avgLatencyMs }} ms</template>
                    <template v-if="item.avgTps !== null"> · {{ item.avgTps.toFixed(1) }} tok/s</template>
                  </small>
                </div>
              </div>
            </section>
          </div>
          <ul v-if="overviewStats.errors.length" class="usage-overview-errors">
            <li v-for="item in overviewStats.errors" :key="item.providerId">
              <strong>{{ item.providerName }}</strong>
              <span>{{ item.message }}</span>
            </li>
          </ul>
        </div>
      </a-spin>
      <a-spin v-else :loading="loading">
        <div v-if="!summary || summary.points.length === 0" class="api-key-empty" role="status">
          {{ loading ? '正在读取用量趋势…' : error ? '用量数据未能加载' : '所选时段暂无用量数据' }}
        </div>
        <div v-else class="usage-dashboard">
          <div class="usage-summary-cards">
            <div v-for="card in usageStatCards" :key="card.key" class="usage-summary-card" :title="card.hint || card.value">
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
                <span>{{ usagePeriodTitle }} · {{ usagePeriodGranularity }}</span>
              </div>
              <div class="usage-chart-legend">
                <i />
                <span>总消耗 {{ formatQuotaValue(usageTotal, summary.quotaDisplay, 6) }}</span>
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
                  <text x="46" :y="label.y + 4">{{ formatQuotaValue(label.value, summary.quotaDisplay, 6) }}</text>
                </g>
              </g>
              <path class="usage-area" :d="usageAreaPath" />
              <path class="usage-line" :d="usageLinePath" />
              <g v-for="point in usageChartPoints" :key="`${point.date}-dot`">
                <line class="usage-column" :x1="point.x" :x2="point.x" y1="182" :y2="point.y" />
                <circle class="usage-dot" :cx="point.x" :cy="point.y" r="3.4">
                  <title>{{ point.date }} · {{ formatQuotaValue(point.used, summary.quotaDisplay, 6) }} · {{ point.requestCount.toLocaleString() }} 次请求 · {{ point.tokenUsed.toLocaleString() }} Tokens</title>
                </circle>
              </g>
              <g v-if="usagePeakPoint" class="usage-peak">
                <circle :cx="usagePeakPoint.x" :cy="usagePeakPoint.y" r="5" />
                <text
                  :x="Math.min(usagePeakPoint.x + 8, 464)"
                  :y="Math.max(usagePeakPoint.y - 8, 14)"
                >
                  峰值 {{ formatQuotaValue(usagePeakPoint.used, summary.quotaDisplay, 6) }}
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
            <details class="usage-point-details">
              <summary>查看时段明细 · {{ summary.points.length }} 个时段</summary>
              <div class="usage-list">
              <div v-for="point in [...summary.points].reverse()" :key="point.date" class="usage-row">
                <span class="usage-date">{{ point.date }}</span>
                <div class="usage-bar-track">
                  <div class="usage-bar" :style="{ width: barPercent(point.used, maxUsageValue) }" />
                </div>
                <strong>{{ formatQuotaValue(point.used, summary.quotaDisplay, 6) }}</strong>
                <span class="usage-meta">{{ point.requestCount.toLocaleString() }} 次 · {{ formatNumberCompact(point.tokenUsed, 0) }} Tokens</span>
              </div>
              </div>
            </details>
          </div>
          <section v-if="usageModelHealth.length" class="usage-model-panel usage-health-panel">
            <div class="usage-model-panel-header">
              <strong>响应质量</strong>
              <span>站点近 24 小时采样 · 成功率低的在前</span>
            </div>
            <div class="usage-model-list">
              <div v-for="item in usageModelHealth" :key="`health-${item.modelName}`" class="usage-model-row">
                <div class="usage-model-row-head">
                  <strong :title="item.modelName">{{ item.modelName }}</strong>
                  <span v-if="item.successRate !== null" :class="healthTone(item.successRate)">
                    成功率 {{ formatPercent(item.successRate) }}
                  </span>
                  <span v-else>成功率未采样</span>
                </div>
                <small>
                  <template v-if="item.avgLatencyMs !== null">平均耗时 {{ item.avgLatencyMs }} ms</template>
                  <template v-if="item.avgTps !== null"> · {{ item.avgTps.toFixed(1) }} tok/s</template>
                  <template v-if="item.requestCount > 0"> · 同一周期 {{ formatNumberCompact(item.requestCount, 0) }} 次</template>
                </small>
              </div>
            </div>
          </section>
          <div v-if="usageTopQuotaModels.length || usageTopCallModels.length" class="usage-model-dashboard">
            <section v-if="usageTopQuotaModels.length" class="usage-model-panel">
              <div class="usage-model-panel-header">
                <strong>模型额度排行</strong>
                <span>按消耗额度排序 · 前 {{ usageTopQuotaModels.length }} 个</span>
              </div>
              <div class="usage-model-list">
                <div v-for="item in usageTopQuotaModels" :key="`quota-${modelName(item)}`" class="usage-model-row">
                  <div class="usage-model-row-head">
                    <strong :title="modelName(item)">{{ modelName(item) }}</strong>
                    <span>{{ formatQuotaValue(item.used, summary.quotaDisplay, 6) }}</span>
                  </div>
                  <div class="usage-model-bar-track">
                    <div class="usage-model-bar usage-model-bar-quota" :style="{ width: modelQuotaPercent(item) }" />
                  </div>
                  <small>{{ formatNumberCompact(item.requestCount, 0) }} 次 · {{ formatNumberCompact(item.tokenUsed, 0) }} tokens</small>
                </div>
              </div>
            </section>
            <section v-if="usageTopCallModels.length" class="usage-model-panel">
              <div class="usage-model-panel-header">
                <strong>模型调用分析</strong>
                <span>按请求次数排序 · 前 {{ usageTopCallModels.length }} 个</span>
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
                  <small>{{ formatQuotaValue(item.used, summary.quotaDisplay, 6) }} · {{ formatNumberCompact(item.tokenUsed, 0) }} tokens</small>
                </div>
              </div>
            </section>
          </div>
        </div>
      </a-spin>
    </div>
  </a-modal>
</template>
