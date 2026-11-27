<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  IconApps,
  IconCloud,
  IconCopy,
  IconDashboard,
  IconList,
  IconRefresh,
  IconSearch,
  IconSwap,
} from "@arco-design/web-vue/es/icon";
import type { AgentCliKind, Provider, ProviderModelPerformance } from "../stores/providers";
import type { ModelMetadata } from "../api/model-catalog";
import { providerDisplayLabel, providerModelScopeLabel } from "../utils/provider-display";
import {
  modelPriceComparison,
  modelPriceDetail,
  modelPriceSourceLabel,
  modelPriceSummary,
  modelPriceTierDetail,
  modelTimePricingDetail,
  pricingScopeNote,
} from "../utils/model-price";
import {
  formatLatency,
  formatSuccessRate,
  formatThroughput,
  modelComparisonProviders,
  providerModelPerformance,
} from "../utils/model-comparison";
import { useModelComparison } from "../composables/useModelComparison";
import { useModelMetadata } from "../composables/useModelMetadata";
import ModelComparisonPanel from "./ModelComparisonPanel.vue";
import ModelMetadataLine from "./ModelMetadataLine.vue";
import "../styles/modules/available-models.css";

const MODELS_PER_PAGE = 100;
type ModelSort = "name" | "availability" | "tps" | "latency";
type ModelView = "rows" | "cards";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  providers?: readonly Provider[];
  loading: boolean;
  error: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  copy: [model: string];
  copyAll: [models: string[]];
  switch: [provider: Provider, cliKind: AgentCliKind, model: string];
  launch: [provider: Provider, cliKind: AgentCliKind, model: string];
}>();

const keyword = ref("");
const page = ref(1);
const sortMode = ref<ModelSort>("name");
const viewMode = ref<ModelView>("rows");
const onlyUnhealthy = ref(false);
const modelList = ref<HTMLElement | null>(null);
/// 比价面板的落点动作：把正在比较的模型写入所选 Agent 的默认配置。先收起
/// 模型窗口，避免默认配置编辑器叠在两层弹窗下面。
function switchComparisonProvider(providerId: string, cliKind: AgentCliKind) {
  const provider = (props.providers ?? []).find((item) => item.identity.id === providerId);
  const model = comparisonModel.value;
  if (!provider || !model) return;
  emit("update:visible", false);
  emit("switch", provider, cliKind, model);
}

/// 同一个比较模型也可以只启动一次，不写默认配置；启动选择器会带着模型
/// 打开，用户在启动前仍可改工作目录和运行参数。
function launchComparisonProvider(providerId: string, cliKind: AgentCliKind) {
  const provider = (props.providers ?? []).find((item) => item.identity.id === providerId);
  const model = comparisonModel.value;
  if (!provider || !model) return;
  emit("update:visible", false);
  emit("launch", provider, cliKind, model);
}

const {
  model: comparisonModel,
  rows: comparisonRows,
  loading: comparisonLoading,
  error: comparisonError,
  open: openComparison,
  close: closeComparison,
} = useModelComparison({
  providers: () => props.providers ?? [],
  currentProviderId: () => props.provider?.identity.id,
});

watch(
  [() => props.visible, () => props.provider?.identity.id],
  () => {
    keyword.value = "";
    page.value = 1;
    sortMode.value = "name";
    viewMode.value = "rows";
    onlyUnhealthy.value = false;
    closeComparison();
  },
);

const modelScope = computed(() => props.provider?.capabilities.availableModelsState.scope
  ?? props.provider?.actions.models.scope);
const scopeHint = computed(() => modelScope.value === "account"
  ? "账号模型包含多个可用分组；当前 Key 能调用的模型仍受其分组和模型限制约束。"
  : "当前 API Key 的模型列表。模型是否可调用还取决于站点的实时服务状态。");
const modalTitle = computed(() => props.provider
  ? `${providerDisplayLabel(props.provider)} · ${providerModelScopeLabel(modelScope.value)}`
  : "可用模型");

const allModels = computed(() =>
  Array.from(
    new Set(
      (props.provider?.capabilities.availableModels ?? [])
        .map((model) => model.trim())
        .filter(Boolean),
    ),
  ),
);
// 模型选择器展示卡片规则（include / exclude / hidden）过滤后的列表；
// 原始列表仍完整保留，被隐藏的数量在这里说明。
const models = computed(() =>
  Array.from(
    new Set(
      (props.provider?.visibleModels ?? [])
        .map((model) => model.trim())
        .filter(Boolean),
    ),
  ).sort((left, right) => left.localeCompare(right)),
);
const hiddenCount = computed(() =>
  Math.max(0, allModels.value.length - models.value.length),
);

const performanceState = computed(() =>
  props.provider?.capabilities.optional?.find((state) => state.capability === "performance"),
);
const performanceCoverage = computed(() => {
  return models.value.reduce(
    (count, model) => count + (performanceFor(model) ? 1 : 0),
    0,
  );
});
const hasPerformance = computed(() => performanceCoverage.value > 0);
const unhealthyModels = computed(() =>
  models.value.filter((model) => {
    const rate = performanceFor(model)?.successRate;
    return rate != null && Number.isFinite(rate) && rate < 95;
  }),
);
const availabilityRange = computed(() => {
  const rates = models.value
    .map((model) => performanceFor(model)?.successRate)
    .filter((rate): rate is number => rate != null && Number.isFinite(rate));
  if (!rates.length) return "";
  const min = Math.min(...rates);
  const max = Math.max(...rates);
  return min === max
    ? formatSuccessRate(min)
    : `${formatSuccessRate(min)}–${formatSuccessRate(max)}`;
});
const sourceModels = computed(() =>
  onlyUnhealthy.value ? unhealthyModels.value : models.value,
);
watch(unhealthyModels, (models) => {
  if (models.length === 0) onlyUnhealthy.value = false;
});

function performanceFor(model: string): ProviderModelPerformance | undefined {
  const provider = props.provider;
  return provider ? providerModelPerformance(provider, model) : undefined;
}

/// 排序只改变列表顺序，缺样本的模型始终排在最后，不把「没有流量」当成差评。
const sortedModels = computed(() => {
  if (sortMode.value === "name" || !hasPerformance.value) return sourceModels.value;
  const metric = (model: string) => {
    const value = performanceFor(model);
    if (!value) return Number.NEGATIVE_INFINITY;
    if (sortMode.value === "availability") return value.successRate ?? Number.NEGATIVE_INFINITY;
    if (sortMode.value === "tps") return value.avgTps ?? Number.NEGATIVE_INFINITY;
    return value.avgLatencyMs == null ? Number.NEGATIVE_INFINITY : -value.avgLatencyMs;
  };
  return [...sourceModels.value].sort((left, right) => {
    const leftMetric = metric(left);
    const rightMetric = metric(right);
    if (leftMetric === rightMetric) return left.localeCompare(right);
    return rightMetric - leftMetric;
  });
});

const filteredModels = computed(() => {
  const filter = keyword.value.trim().toLowerCase();
  const source = sortedModels.value;
  if (!filter) return source;
  return source.filter((model) => model.toLowerCase().includes(filter));
});

function formatCapacity(value: number | null | undefined) {
  if (value == null) return "";
  return value >= 1_000_000
    ? `${+(value / 1_000_000).toFixed(2)}M`
    : value >= 1000
      ? `${+(value / 1000).toFixed(1)}K`
      : String(value);
}

interface PriceBadge {
  label: string;
  tone: "good" | "warn" | "bad" | "neutral";
  title: string;
}

function priceBadges(model: ModelMetadata | undefined): PriceBadge[] {
  if (!model) return [];
  const badges: PriceBadge[] = [];
  const comparison = modelPriceComparison(model);
  if (comparison && comparison.tone !== "neutral") {
    badges.push({
      label: comparison.label,
      tone: comparison.tone,
      title: comparison.detail,
    });
  }
  if (model.priceTiers.length) {
    badges.push({
      label: model.billing === "image" ? "档位" : "阶梯",
      tone: "neutral",
      title: modelPriceTierDetail(model),
    });
  }
  if (model.timePricing) {
    badges.push({
      label: "分时",
      tone: "neutral",
      title: modelTimePricingDetail(model),
    });
  }
  return badges;
}

function priceTitle(model: ModelMetadata | undefined) {
  return [
    modelPriceSummary(model),
    modelPriceSourceLabel(model),
    modelPriceDetail(model),
  ]
    .filter(Boolean)
    .join(" · ");
}

function setSort(mode: ModelSort) {
  sortMode.value = mode;
}

function successRateTone(value: number | null | undefined) {
  if (value == null) return "";
  if (value >= 99.5) return "is-good";
  if (value >= 95) return "is-warn";
  return "is-bad";
}

const displayedModels = computed(() => filteredModels.value.slice((page.value - 1) * MODELS_PER_PAGE, page.value * MODELS_PER_PAGE));
const metadata = useModelMetadata(
  displayedModels,
  computed(() => props.visible),
  computed(() => props.provider?.identity.id),
  computed(() => props.provider?.auth.credentialRevision),
);
const modelRows = computed(() =>
  displayedModels.value.map((name) => {
    const modelMetadata = metadata.models.value.get(name);
    return {
      name,
      performance: performanceFor(name),
      metadata: modelMetadata,
      price: modelPriceSummary(modelMetadata),
      comparison: modelPriceComparison(modelMetadata),
      badges: priceBadges(modelMetadata),
      priceTitle: priceTitle(modelMetadata),
      comparisonCount: modelComparisonProviders(props.providers ?? [], name).length,
    };
  }),
);
watch(keyword, () => { page.value = 1; });
watch(sortMode, () => { page.value = 1; });
watch(viewMode, () => { page.value = 1; });
watch(onlyUnhealthy, () => { page.value = 1; });
watch(() => filteredModels.value.length, (count) => { page.value = Math.min(page.value, Math.max(1, Math.ceil(count / MODELS_PER_PAGE))); });
watch([page, keyword, viewMode, onlyUnhealthy], () => { if (modelList.value) modelList.value.scrollTop = 0; }, { flush: "post" });

const canRefresh = computed(() => Boolean(props.provider?.actions.models.canSync));

const priceScopeNote = computed(() =>
  pricingScopeNote(props.provider?.capabilities.pricing?.groupPricing),
);
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal available-models-modal"
    :footer="false"
    :width="720"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title available-models-title">
        <span class="surface-modal-title-icon"><icon-cloud /></span>
        <span class="surface-modal-title-copy">
          <strong :title="scopeHint">{{ modalTitle }}</strong>
        </span>
      </div>
    </template>
    <div class="available-models-panel">
      <template v-if="!comparisonModel">
      <a-alert v-if="modelScope === 'account'" :type="provider?.capabilities.availableModelsState.fallbackReason ? 'warning' : 'info'" show-icon>
        <template v-if="provider?.capabilities.availableModelsState.fallbackReason">
          当前 Key 模型接口失败，已改用账号模型。{{ provider.capabilities.availableModelsState.fallbackReason }}
        </template>
        <template v-else>尚未配置可用的 API Key，当前显示账号模型。</template>
        <div>{{ scopeHint }}</div>
      </a-alert>
      <a-alert
        v-if="provider && !canRefresh"
        type="warning"
      >
        {{ provider.actions.models.unavailableReason }}
      </a-alert>

      <div class="available-models-toolbar">
        <a-input v-model="keyword" allow-clear placeholder="搜索模型名称" aria-label="搜索模型名称">
          <template #prefix><icon-search /></template>
        </a-input>
        <a-radio-group
          v-model="viewMode"
          class="available-models-view-switch"
          type="button"
          size="small"
          aria-label="模型视图"
        >
          <a-radio value="rows" title="列表视图" aria-label="列表视图">
            <icon-list />
          </a-radio>
          <a-radio value="cards" title="卡片视图" aria-label="卡片视图">
            <icon-apps />
          </a-radio>
        </a-radio-group>
        <a-button :disabled="filteredModels.length === 0" @click="emit('copyAll', filteredModels)">
          <template #icon><icon-copy /></template>
          {{ keyword.trim() || onlyUnhealthy ? '复制筛选结果' : '复制全部' }}
        </a-button>
        <a-button type="primary" :loading="loading" :disabled="!canRefresh" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
      </div>
      <a-alert v-if="error" type="error" show-icon>
        <div class="panel-load-error"><span>{{ error }}</span><a-button size="small" :disabled="!canRefresh" @click="emit('refresh')">重试</a-button></div>
      </a-alert>

      <section v-if="hasPerformance" class="available-models-performance" aria-label="模型表现">
        <div class="available-models-performance-copy">
          <span class="available-models-performance-heading"><icon-dashboard /> 模型表现</span>
          <span>站点近 24 小时请求统计 · {{ performanceCoverage.toLocaleString() }} 个模型有样本<template v-if="availabilityRange"> · 可用性 {{ availabilityRange }}</template></span>
        </div>
        <div class="available-models-performance-controls">
          <a-radio-group v-model="sortMode" type="button" size="small" aria-label="按模型表现排序">
            <a-radio value="name" title="按名称排序">名称</a-radio>
            <a-radio value="availability" title="可用性从高到低">可用性</a-radio>
            <a-radio value="tps" title="TPS 从高到低">TPS</a-radio>
            <a-radio value="latency" title="延迟从低到高">延迟</a-radio>
          </a-radio-group>
          <a-checkbox
            v-if="unhealthyModels.length"
            v-model="onlyUnhealthy"
            class="available-models-unhealthy-toggle"
            title="站点成功率低于 95% 的模型，可能是上游线路或模型本身异常"
          >仅看可用性低于 95%（{{ unhealthyModels.length }}）</a-checkbox>
        </div>
      </section>
      <p
        v-else-if="performanceState?.supported"
        class="available-model-performance-note"
      >
        站点近 24 小时还没有可用的模型表现样本。
      </p>
      <p
        v-else-if="performanceState && !performanceState.known && performanceState.error"
        class="available-model-performance-note"
      >
        模型表现暂时无法读取：{{ performanceState.error }}
      </p>

      <a-spin :loading="loading">
        <div v-if="models.length === 0" class="available-models-empty">
          {{ loading ? '正在获取模型列表…' : error ? '模型列表未能加载' : hiddenCount ? `展示规则隐藏了全部 ${hiddenCount} 个模型` : provider?.capabilities.availableModelsState.updatedAt ? '暂无可用模型' : '暂未获取模型列表' }}
        </div>
        <div v-else class="available-models-body">
          <div class="available-models-summary">
            <span>{{ keyword.trim() ? `匹配 ${filteredModels.length.toLocaleString()} 个 · 共 ${models.length.toLocaleString()} 个模型` : `共 ${models.length.toLocaleString()} 个模型` }}<template v-if="hiddenCount">（展示规则隐藏 {{ hiddenCount }} 个）</template></span>
            <span>{{ (providers?.length ?? 0) > 1 ? "点击模型名称即可复制；价格列 ⇄ 可跨站比价" : "点击模型名称即可复制" }}</span>
          </div>
          <p v-if="priceScopeNote" class="available-models-price-scope">
            价格口径：{{ priceScopeNote }}
          </p>
          <div v-if="filteredModels.length === 0" class="available-models-empty">
            <span>{{ onlyUnhealthy && !keyword.trim() ? "没有可用性低于 95% 的模型" : "没有匹配的模型" }}</span>
            <a-button type="text" @click="keyword = ''; onlyUnhealthy = false">清除筛选</a-button>
          </div>
          <div
            v-else-if="viewMode === 'rows'"
            ref="modelList"
            class="available-models-table-wrap"
          >
            <table class="available-models-table" aria-label="可用模型列表">
              <thead>
                <tr>
                  <th
                    scope="col"
                    class="available-models-table-model"
                    :aria-sort="sortMode === 'name' ? 'ascending' : 'none'"
                  >
                    <button
                      type="button"
                      class="available-models-table-sort"
                      :class="{ active: sortMode === 'name', 'is-asc': sortMode === 'name' }"
                      @click="setSort('name')"
                    >模型</button>
                  </th>
                  <template v-if="hasPerformance">
                    <th scope="col" class="available-models-table-metric" :aria-sort="sortMode === 'availability' ? 'descending' : 'none'">
                      <button
                        type="button"
                        class="available-models-table-sort"
                        :class="{ active: sortMode === 'availability' }"
                        @click="setSort('availability')"
                      >可用性</button>
                    </th>
                    <th scope="col" class="available-models-table-metric" :aria-sort="sortMode === 'tps' ? 'descending' : 'none'">
                      <button
                        type="button"
                        class="available-models-table-sort"
                        :class="{ active: sortMode === 'tps' }"
                        @click="setSort('tps')"
                      >TPS</button>
                    </th>
                    <th scope="col" class="available-models-table-metric" :aria-sort="sortMode === 'latency' ? 'ascending' : 'none'">
                      <button
                        type="button"
                        class="available-models-table-sort"
                        :class="{ active: sortMode === 'latency', 'is-asc': sortMode === 'latency' }"
                        @click="setSort('latency')"
                      >延迟</button>
                    </th>
                  </template>
                  <th scope="col" class="available-models-table-spec" title="上下文窗口 / 最大输出，来自站点价目或 models.dev">上下文 / 输出</th>
                  <th
                    scope="col"
                    class="available-models-table-price"
                    title="输入 / 输出每百万 token 价格；按次显示 /次，图片按张显示 /张。角标对比官方参考价、长上下文阶梯和分时倍率"
                  >价格</th>
                  <th scope="col" class="available-models-table-copy" aria-label="模型操作"></th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in modelRows" :key="row.name">
                  <td class="available-models-table-model">
                    <button
                      type="button"
                      class="available-model-table-name"
                      :title="`${row.name} · 点击复制`"
                      @click="emit('copy', row.name)"
                    >{{ row.name }}</button>
                    <ModelMetadataLine :model="row.metadata" />
                  </td>
                  <template v-if="hasPerformance">
                    <td class="available-models-table-metric">
                      <span
                        v-if="row.performance?.successRate != null"
                        :class="successRateTone(row.performance.successRate)"
                      >{{ formatSuccessRate(row.performance.successRate) }}</span>
                      <span v-else class="available-models-table-empty">—</span>
                      <span
                        v-if="row.performance?.recentSuccess.length"
                        class="available-model-uptime"
                        title="近期可用性采样"
                        aria-hidden="true"
                      ><i
                        v-for="point in row.performance.recentSuccess.slice(-14)"
                        :key="point.ts"
                        :class="successRateTone(point.successRate)"
                      /></span>
                    </td>
                    <td class="available-models-table-metric">
                      <span v-if="row.performance?.avgTps != null">{{ formatThroughput(row.performance.avgTps) }}</span>
                      <span v-else class="available-models-table-empty">—</span>
                    </td>
                    <td class="available-models-table-metric">
                      <span v-if="row.performance?.avgLatencyMs != null">{{ formatLatency(row.performance.avgLatencyMs) }}</span>
                      <span v-else class="available-models-table-empty">—</span>
                    </td>
                  </template>
                  <td class="available-models-table-spec">
                    <template v-if="row.metadata && (row.metadata.contextWindow != null || row.metadata.outputLimit != null)">
                      {{ formatCapacity(row.metadata.contextWindow) || "—" }} / {{ formatCapacity(row.metadata.outputLimit) || "—" }}
                    </template>
                    <span v-else class="available-models-table-empty">—</span>
                  </td>
                  <td class="available-models-table-price" :title="row.priceTitle">
                    <span v-if="row.price" class="available-model-price-main">{{ row.price }}</span>
                    <span v-else class="available-models-table-empty">—</span>
                    <span v-if="row.badges.length" class="available-model-price-badges">
                      <span
                        v-for="badge in row.badges"
                        :key="badge.label"
                        class="available-model-price-badge"
                        :class="`is-${badge.tone}`"
                        :title="badge.title"
                      >{{ badge.label }}</span>
                    </span>
                  </td>
                  <td class="available-models-table-copy">
                    <button
                      v-if="row.comparisonCount > 1"
                      type="button"
                      class="available-model-table-compare"
                      :aria-label="`对比 ${row.name} 的跨站价格`"
                      title="跨站比价"
                      @click="openComparison(row.name)"
                    ><icon-swap /></button>
                    <button
                      type="button"
                      class="available-model-table-copy"
                      :aria-label="`复制 ${row.name}`"
                      title="复制模型名"
                      @click="emit('copy', row.name)"
                    ><icon-copy /></button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <div v-else ref="modelList" class="available-models-list">
            <div
              v-for="row in modelRows"
              :key="row.name"
              class="available-model-card"
              :class="{ 'has-comparison': row.comparisonCount > 1 }"
            >
              <button
                type="button"
                class="available-model-item"
                :title="row.name"
                @click="emit('copy', row.name)"
              >
                <span class="available-model-copy">
                  <span>{{ row.name }}</span>
                  <span v-if="row.performance" class="available-model-performance">
                    <span
                      v-if="row.performance.successRate != null"
                      :class="successRateTone(row.performance.successRate)"
                      title="站点观测到的请求成功率，不含业务拒绝"
                    >可用性 {{ formatSuccessRate(row.performance.successRate) }}</span>
                    <span
                      v-if="row.performance.recentSuccess.length"
                      class="available-model-uptime"
                      title="近期可用性采样"
                      aria-hidden="true"
                    ><i
                      v-for="point in row.performance.recentSuccess.slice(-24)"
                      :key="point.ts"
                      :class="successRateTone(point.successRate)"
                    /></span>
                    <span
                      v-if="row.performance.avgTps != null"
                      title="站点观测到的输出吞吐"
                    >{{ formatThroughput(row.performance.avgTps) }}</span>
                    <span
                      v-if="row.performance.avgLatencyMs != null"
                      title="站点观测到的平均延迟"
                    >{{ formatLatency(row.performance.avgLatencyMs) }}</span>
                  </span>
                  <span v-if="row.price" class="available-model-price" :title="row.priceTitle">
                    {{ row.price }}<em
                      v-if="row.comparison && row.comparison.tone !== 'neutral'"
                      :class="`is-${row.comparison.tone}`"
                    >{{ row.comparison.label }}</em><em
                      v-if="row.metadata?.priceTiers.length"
                    >{{ row.metadata.billing === "image" ? "档位" : "阶梯" }}</em><em
                      v-if="row.metadata?.timePricing"
                    >分时</em>
                  </span>
                  <ModelMetadataLine :model="row.metadata" />
                </span>
                <icon-copy v-if="row.comparisonCount <= 1" />
              </button>
              <button
                v-if="row.comparisonCount > 1"
                type="button"
                class="available-model-compare"
                :aria-label="`对比 ${row.name} 的跨站价格`"
                title="跨站比价"
                @click="openComparison(row.name)"
              ><icon-swap /></button>
            </div>
          </div>
          <div v-if="filteredModels.length > MODELS_PER_PAGE" class="available-models-pagination">
            <span>每页 {{ MODELS_PER_PAGE }} 个</span>
            <a-pagination v-model:current="page" :total="filteredModels.length" :page-size="MODELS_PER_PAGE" size="small" simple />
          </div>
          <p class="available-model-metadata-note"><template v-if="metadata.loading.value">正在补充模型资料…</template><template v-else-if="metadata.error.value">模型资料暂不可用，不影响模型选择。<a-button type="text" size="mini" @click="metadata.refresh">重试</a-button></template><template v-else-if="metadata.snapshot.value?.updatedAt">模型资料来自 models.dev，价格优先使用站点价目；实际能力与费用以中转站为准。</template></p>
        </div>
      </a-spin>
      </template>
      <template v-else>
        <ModelComparisonPanel
          :model="comparisonModel"
          :rows="comparisonRows"
          :loading="comparisonLoading"
          :error="comparisonError"
          @back="closeComparison"
          @refresh="openComparison(comparisonModel)"
          @switch="switchComparisonProvider"
          @launch="launchComparisonProvider"
        />
      </template>
    </div>
  </a-modal>
</template>

<style scoped>
.available-model-copy { display: flex; flex-direction: column; gap: 4px; min-width: 0; text-align: left; }
.available-model-copy > span:first-child { overflow: hidden; text-overflow: ellipsis; }
.available-model-metadata-note { color: var(--color-text-3); margin: 8px 0 0; font-size: 11px; }
</style>
