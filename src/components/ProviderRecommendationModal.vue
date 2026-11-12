<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { ArrowRight, RefreshCw, Sparkles } from "@lucide/vue";
import type { AgentCliKind, Provider } from "../stores/provider-types";
import type { ProviderRecommendationReport } from "../stores/provider-recommendation-types";
import { getProviderRecommendations } from "../api/provider-recommendation";
import { withTimeout } from "../utils/promise-timeout";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import "../styles/modules/provider-recommendation.css";

const props = defineProps<{ visible: boolean; providers: readonly Provider[] }>();
const emit = defineEmits<{
  close: [];
  switch: [provider: Provider, cliKind: AgentCliKind];
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
    if (!options.some((option) => option.kind === selectedCli.value)) {
      selectedCli.value = options[0]?.kind ?? "";
    }
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

async function load() {
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
  emit("switch", provider, selectedCli.value);
  emit("close");
}

function percentLabel(value: number | null) {
  return value === null ? "" : `${value.toFixed(0)}%`;
}

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      request += 1;
      loading.value = false;
      return;
    }
    void load();
  },
  { immediate: true },
);
watch(model, scheduleLoad);
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
        按余额、额度窗口剩余与重置时间、最近 24 次测活成功率和模型价格排序；只读取现有卡片数据，不发起额外请求。
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
              </div>
              <div class="provider-recommendation-facts">
                <span>{{ entry.quotaLabel }}</span>
                <span v-if="entry.livenessSamples">
                  测活 {{ percentLabel(entry.livenessSuccessRate) }}（近 {{ entry.livenessSamples }} 次）
                </span>
                <span v-else>暂无测活记录</span>
                <span v-if="entry.inputPrice !== null">输入 ${{ entry.inputPrice.toFixed(2) }}/M</span>
                <span v-if="entry.outputPrice !== null">输出 ${{ entry.outputPrice.toFixed(2) }}/M</span>
                <span v-if="entry.modelSupported === true">模型可用</span>
                <span v-else-if="entry.modelSupported === false">模型不在列表</span>
                <span v-else-if="model">模型列表未同步</span>
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
