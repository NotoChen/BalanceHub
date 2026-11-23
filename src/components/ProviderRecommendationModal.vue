<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { ArrowRight, RefreshCw, Sparkles } from "@lucide/vue";
import type { AgentCliKind, Provider } from "../stores/provider-types";
import type { ProviderRecommendationReport } from "../stores/provider-recommendation-types";
import { getProviderRecommendations } from "../api/provider-recommendation";
import { withTimeout } from "../utils/promise-timeout";
import type { RecommendationAgentContext } from "../utils/provider-recommendation-context.ts";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import "../styles/modules/provider-recommendation.css";

const props = defineProps<{
  visible: boolean;
  providers: readonly Provider[];
  /// 从 Agent 上下文打开时带上它的当前绑定：默认目标、默认模型和「当前」
  /// 标记都从这里来，用户不必再手动找回自己是哪个 Agent。
  agent?: RecommendationAgentContext | null;
}>();
const emit = defineEmits<{
  close: [];
  switch: [provider: Provider, cliKind: AgentCliKind, model: string];
}>();

const cliStore = useCliRuntimeStore();
const model = ref("");
const loading = ref(false);
const error = ref("");
const report = ref<ProviderRecommendationReport | null>(null);
const selectedCli = ref<AgentCliKind | "">("");
let request = 0;
let debounce: ReturnType<typeof setTimeout> | undefined;

const modelOptions = computed(() => {
  const models = new Set<string>();
  for (const provider of props.providers) {
    for (const item of provider.visibleModels ?? []) models.add(item);
    for (const item of Object.keys(provider.modelConfig?.overrides ?? {})) models.add(item);
    if (provider.liveness?.model) models.add(provider.liveness.model);
    if (provider.cli?.preferredModel) models.add(provider.cli.preferredModel);
  }
  return [...models].filter(Boolean).sort((left, right) => left.localeCompare(right));
});

const cliOptions = computed(() =>
  (cliStore.cliEnvironmentProbe?.tools ?? []).filter((tool) => tool.available),
);
watch(
  cliOptions,
  (options) => {
    // 只修正失效的选择。探测结果每 30 秒会被父组件替换一次，不能借机把
    // 用户在「切换目标」里手动选中的 CLI 改回 Agent 上下文。
    if (options.some((option) => option.kind === selectedCli.value)) return;
    const contextKind = props.agent?.kind;
    selectedCli.value =
      contextKind && options.some((option) => option.kind === contextKind)
        ? contextKind
        : (options[0]?.kind ?? "");
  },
  { immediate: true },
);

const summary = computed(() => {
  const current = report.value;
  if (!current) return "";
  const eligible = current.entries.filter((entry) => entry.eligible).length;
  return `${eligible} / ${current.entries.length} 个账号可用`;
});

const checkedAt = computed(() => {
  const value = report.value?.generatedAt;
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString("zh-CN", { hour12: false });
});

/// 站点按次单价通常小于 $0.01，固定两位会显示成 0.00。
function priceAmount(value: number, digits = 2) {
  return +value.toFixed(digits);
}

/// 打开时从 Agent 上下文带进来的模型只负责这一次加载；消费掉标记后，
/// 后续用户改模型仍走正常的防抖重算。
let contextModelChange = false;

async function load() {
  // 关闭后仍在排队的防抖计算不能落到下一次会话，也不能为已关闭的窗口
  // 发起一次无人接收的请求。
  if (!props.visible) return;
  const id = ++request;
  loading.value = true;
  error.value = "";
  try {
    const result = await withTimeout(
      getProviderRecommendations(model.value),
      20_000,
      "生成中转站推荐超时",
    );
    if (id !== request) return;
    report.value = result;
  } catch (cause) {
    if (id !== request) return;
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    if (id === request) loading.value = false;
  }
}

function scheduleLoad() {
  if (debounce) clearTimeout(debounce);
  if (!props.visible) return;
  debounce = setTimeout(() => {
    debounce = undefined;
    void load();
  }, 350);
}

function refresh() {
  if (debounce) {
    clearTimeout(debounce);
    debounce = undefined;
  }
  void load();
}

function switchTo(providerId: string) {
  const provider = props.providers.find((item) => item.identity.id === providerId);
  if (!provider || !selectedCli.value) return;
  // The report ranks a concrete model. Pass it through so the write does not
  // silently land on the provider's global preferred model instead.
  emit("switch", provider, selectedCli.value, model.value.trim());
  emit("close");
}

function percentLabel(value: number | null) {
  return value === null ? "" : `${value.toFixed(0)}%`;
}

/// 后端时间字段同时存在毫秒数字串、秒数字串和 RFC3339，展示时统一成短时间。
function timestampLabel(value: string | number | null) {
  if (value === null || value === "") return "时间未知";
  const numeric = typeof value === "number" ? value : Number(value);
  const date = Number.isFinite(numeric)
    ? new Date(numeric > 1_000_000_000_000 ? numeric : numeric * 1000)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return "时间未知";
  return date.toLocaleString("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/// 站点成功率保留一位小数，避免 99.5% 被四舍五入成 100%。
function sitePercentLabel(value: number) {
  return `${(value * 100).toFixed(1)}%`;
}

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      request += 1;
      loading.value = false;
      return;
    }
    const agent = props.agent;
    if (agent?.kind) {
      // Agent 上下文优先：用户是从某个 Agent 的推荐入口进来的，默认目标
      // 就应该是它；只有这个 CLI 不可用时才回落到第一个可用项。
      selectedCli.value = cliOptions.value.some(
        (option) => option.kind === agent.kind,
      )
        ? agent.kind
        : (cliOptions.value[0]?.kind ?? "");
    }
    if (agent?.model && model.value !== agent.model) {
      contextModelChange = true;
      model.value = agent.model;
    }
    // 关闭期间 Agent 绑定可能已经换了模型；重新打开时不能把旧模型的
    // 排名留在新模型下面。
    const requestedModel = model.value.trim();
    const shownModel = report.value?.model ?? null;
    if (shownModel !== null && shownModel !== requestedModel) {
      report.value = null;
    }
    void load();
  },
  { immediate: true },
);
watch(model, () => {
  // 模型一变，旧模型的排名立即失效：已经显示的报告和仍在读取的请求
  // 都要作废。否则用户会在新模型下按旧排名点「切换」，把新模型写到
  // 为旧模型选出的站点上。
  request += 1;
  const requestedModel = model.value.trim();
  const shownModel = report.value?.model ?? null;
  if (shownModel !== null && shownModel !== requestedModel) {
    report.value = null;
  }
  if (contextModelChange) {
    // Agent 上下文带入的模型由打开流程负责发起读取。
    contextModelChange = false;
    return;
  }
  if (props.visible) {
    loading.value = true;
    scheduleLoad();
  }
}, { flush: "sync" });
onBeforeUnmount(() => {
  request += 1;
  if (debounce) clearTimeout(debounce);
});
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(820px, calc(100vw - 32px))"
    modal-class="surface-modal provider-recommendation-modal"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="emit('close')"
  >
    <template #title>
      <div class="provider-recommendation-heading">
        <strong>中转站推荐</strong>
        <span v-if="summary" class="provider-recommendation-summary">{{ summary }}</span>
      </div>
    </template>
    <div class="provider-recommendation-body">
      <p class="provider-recommendation-note">
        按余额、额度窗口剩余与重置时间、最近 24 次测活成功率、站点模型表现和模型价格排序；只读取现有卡片数据，不发起额外请求。
      </p>
      <p v-if="agent" class="provider-recommendation-context">
        为 <strong>{{ agent.label }}</strong> 的当前绑定挑选
        <span v-if="agent.model"> · 模型 <code>{{ agent.model }}</code></span>
        <span> · 现在来自 {{ agent.providerLabel }}</span>
      </p>
      <div class="provider-recommendation-controls">
        <a-select
          v-model="model"
          class="provider-recommendation-model"
          :options="modelOptions.map((item) => ({ value: item, label: item }))"
          placeholder="全部模型（综合排序）"
          allow-search
          allow-create
          allow-clear
          :loading="loading"
        >
          <template #prefix><Sparkles :size="14" aria-hidden="true" /></template>
        </a-select>
        <a-button :loading="loading" aria-label="重新生成推荐" @click="refresh">
          <template v-if="!loading" #icon><RefreshCw :size="14" /></template>
          重新计算
        </a-button>
      </div>
      <p v-if="error" class="provider-recommendation-error" role="alert">{{ error }}</p>
      <p v-if="loading && !report" class="provider-recommendation-state" role="status">
        <a-spin :size="14" /> 正在整理余额、测活和价格数据…
      </p>
      <template v-else-if="report">
        <ol v-if="report.entries.length" class="provider-recommendation-list">
          <li
            v-for="(entry, index) in report.entries"
            :key="entry.providerId"
            class="provider-recommendation-entry"
            :class="{ 'is-blocked': !entry.eligible, 'is-top': index === 0 && entry.eligible }"
          >
            <span class="provider-recommendation-rank">{{ index + 1 }}</span>
            <div class="provider-recommendation-content">
              <div class="provider-recommendation-title">
                <strong>{{ entry.label || entry.providerId }}</strong>
                <span v-if="entry.eligible" class="provider-recommendation-score">{{ entry.score }} 分</span>
                <span v-else class="provider-recommendation-score is-blocked">不可用</span>
                <span v-if="index === 0 && entry.eligible" class="provider-recommendation-badge">推荐</span>
                <span
                  v-if="agent && entry.providerId === agent.providerId"
                  class="provider-recommendation-badge is-current"
                >当前</span>
              </div>
              <div class="provider-recommendation-facts">
                <span>{{ entry.quotaLabel }}</span>
                <span v-if="entry.livenessSamples">
                  测活 {{ percentLabel(entry.livenessSuccessRate) }}（近 {{ entry.livenessSamples }} 次）
                </span>
                <span v-else>暂无测活记录</span>
                <span v-if="entry.inputPrice !== null">输入 ${{ priceAmount(entry.inputPrice) }}/M</span>
                <span v-if="entry.outputPrice !== null">输出 ${{ priceAmount(entry.outputPrice) }}/M</span>
                <span v-if="entry.perRequestPrice !== null">按次 ${{ priceAmount(entry.perRequestPrice, 4) }}/次</span>
                <span v-if="entry.modelSupported === true">模型可用</span>
                <span v-else-if="entry.modelSupported === false">模型不在列表</span>
                <span v-else-if="model">模型列表未同步</span>
                <span
                  v-if="entry.siteSuccessRate != null"
                  class="provider-recommendation-site"
                  :class="entry.siteSuccessRate >= 0.995 ? 'is-good' : entry.siteSuccessRate >= 0.95 ? 'is-warn' : 'is-bad'"
                >
                  站点表现 {{ sitePercentLabel(entry.siteSuccessRate) }}
                </span>
                <span v-if="entry.siteAvgTps != null">{{ entry.siteAvgTps.toFixed(1) }} t/s</span>
                <span v-if="entry.siteAvgLatencyMs != null">
                  延迟 {{ entry.siteAvgLatencyMs >= 1000 ? `${(entry.siteAvgLatencyMs / 1000).toFixed(2)}s` : `${Math.round(entry.siteAvgLatencyMs)}ms` }}
                </span>
                <span v-if="entry.cooldownUntil" class="provider-recommendation-data-age">
                  冷却至 {{ timestampLabel(entry.cooldownUntil) }}
                </span>
                <span v-if="entry.quotaStale" class="provider-recommendation-data-age" :title="`余额读取于 ${timestampLabel(entry.quotaUpdatedAt)}`">
                  余额截至 {{ timestampLabel(entry.quotaUpdatedAt) }}
                </span>
                <span v-if="entry.livenessStale" class="provider-recommendation-data-age" :title="`测活记录于 ${timestampLabel(entry.livenessUpdatedAt)}`">
                  测活截至 {{ timestampLabel(entry.livenessUpdatedAt) }}
                </span>
                <span v-if="entry.pricingStale" class="provider-recommendation-data-age" :title="`价格读取于 ${timestampLabel(entry.pricingUpdatedAt)}`">
                  价格截至 {{ timestampLabel(entry.pricingUpdatedAt) }}
                </span>
              </div>
              <ul v-if="entry.blockers.length" class="provider-recommendation-blockers">
                <li v-for="blocker in entry.blockers" :key="blocker">{{ blocker }}</li>
              </ul>
              <p v-else-if="entry.reasons.length" class="provider-recommendation-reasons">
                {{ entry.reasons.join(" · ") }}
              </p>
            </div>
            <a-button
              v-if="entry.eligible"
              class="provider-recommendation-switch"
              size="small"
              :disabled="!selectedCli"
              @click="switchTo(entry.providerId)"
            >
              切换<ArrowRight :size="13" aria-hidden="true" />
            </a-button>
          </li>
        </ol>
        <p v-else class="provider-recommendation-state">还没有可参与排序的中转站。</p>
      </template>
      <p v-else class="provider-recommendation-state">尚未生成推荐。</p>
      <footer class="provider-recommendation-actions">
        <label v-if="cliOptions.length" class="provider-recommendation-cli">
          <span>切换目标</span>
          <a-select
            v-model="selectedCli"
            size="small"
            :options="cliOptions.map((option) => ({ value: option.kind, label: option.label }))"
          />
        </label>
        <span v-else class="provider-recommendation-cli-note">未检测到可用的 Agent CLI，仍可查看排序结果。</span>
        <span v-if="checkedAt" class="provider-recommendation-time">计算时间 {{ checkedAt }}</span>
        <a-button @click="emit('close')">关闭</a-button>
      </footer>
    </div>
  </a-modal>
</template>
