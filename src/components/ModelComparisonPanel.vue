<script setup lang="ts">
import { computed, ref } from "vue";
import { IconArrowLeft, IconRefresh } from "@arco-design/web-vue/es/icon";
import type { ModelComparisonRow } from "../utils/model-comparison";
import type { AgentCliKind } from "../stores/providers";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { agentCliLabel, availableCliKinds } from "../utils/cli-environment";

defineProps<{
  model: string;
  rows: ModelComparisonRow[];
  loading: boolean;
  error: string;
}>();

const emit = defineEmits<{
  back: [];
  refresh: [];
  switch: [providerId: string, cliKind: AgentCliKind];
  launch: [providerId: string, cliKind: AgentCliKind];
}>();

const store = useCliRuntimeStore();
/// 比价面板同时是决策面板：读完各站价格后，既可以把模型写进某个 Agent 的
/// 默认配置，也可以带着这个模型直接启动一次临时 CLI，不用回到卡片重找站点。
const switchableKinds = computed(() =>
  availableCliKinds(store.cliEnvironmentProbe, "defaultConfig"),
);
const launchableKinds = computed(() =>
  availableCliKinds(store.cliEnvironmentProbe, "temporaryLaunch"),
);
/// 两种动作共用同一个目标 Agent 选择；某个 Agent 只支持其中一种时，另一颗
/// 按钮会单独说明原因。
const targetKinds = computed(() =>
  [...new Set([...switchableKinds.value, ...launchableKinds.value])],
);
const targetKind = ref<AgentCliKind | null>(null);
const preferredKind = computed<AgentCliKind | null>(() => {
  const kinds = targetKinds.value;
  if (!kinds.length) return null;
  const chosen = targetKind.value;
  return chosen && kinds.includes(chosen) ? chosen : kinds[0];
});

/// 每行可用的 Agent 由卡片本身决定：OAuth 账号只能交给本家 Agent。
function rowKinds(row: ModelComparisonRow, kinds: AgentCliKind[]) {
  return kinds.filter(
    (kind) => !row.allowedKind || row.allowedKind === kind,
  );
}

function kindFor(row: ModelComparisonRow, kinds: AgentCliKind[]) {
  const allowed = rowKinds(row, kinds);
  const preferred = preferredKind.value;
  return preferred && allowed.includes(preferred) ? preferred : (allowed[0] ?? null);
}

function switchKindFor(row: ModelComparisonRow) {
  return kindFor(row, switchableKinds.value);
}

function launchKindFor(row: ModelComparisonRow) {
  return kindFor(row, launchableKinds.value);
}

function kindLabel(kind: AgentCliKind | null) {
  return kind ? agentCliLabel(store.cliEnvironmentProbe, kind) : "";
}

const activeKindLabel = computed(() => kindLabel(preferredKind.value));

/// 哪几个 Agent 当前就在用这一站：比价时不只看价格，也看切换会不会打断
/// 已经在用的绑定。
function servingLabels(providerId: string) {
  return store.cliRuntime.configs
    .filter((config) => config.providerId === providerId)
    .map((config) => agentCliLabel(store.cliEnvironmentProbe, config.cliKind));
}

function switchBlockedReason(row: ModelComparisonRow) {
  if (row.current) return "这个模型当前就来自此站";
  if (row.disabled) return "该中转站已停用，先在卡片中启用";
  if (!row.canSwitch) return "此卡片不支持写入 Agent 默认配置";
  if (!switchableKinds.value.length) return "本机没有可写入默认配置的 Agent CLI";
  if (!switchKindFor(row)) return `该账号只能写入 ${kindLabel(row.allowedKind)} 的默认配置`;
  return "";
}

function launchBlockedReason(row: ModelComparisonRow) {
  if (row.disabled) return "该中转站已停用，先在卡片中启用";
  if (!row.canLaunch) return "此卡片不支持启动临时 CLI";
  if (!launchableKinds.value.length) return "本机没有可启动的 Agent CLI";
  if (!launchKindFor(row)) return `该账号只能启动 ${kindLabel(row.allowedKind)}`;
  return "";
}

function selectTargetKind(value: unknown) {
  const kind = targetKinds.value.find((item) => item === value);
  if (kind) targetKind.value = kind;
}

function switchTo(row: ModelComparisonRow) {
  const kind = switchKindFor(row);
  if (switchBlockedReason(row) || !kind) return;
  emit("switch", row.providerId, kind);
}

function launchTo(row: ModelComparisonRow) {
  const kind = launchKindFor(row);
  if (launchBlockedReason(row) || !kind) return;
  emit("launch", row.providerId, kind);
}
</script>

<template>
  <div class="available-models-compare">
    <div class="available-models-compare-head">
      <a-button size="small" @click="emit('back')">
        <template #icon><icon-arrow-left /></template>
        返回
      </a-button>
      <div class="available-models-compare-title">
        <strong :title="model">{{ model }}</strong>
        <span>跨站比价 · {{ rows.length }} 个中转站</span>
      </div>
      <a-button size="small" :loading="loading" @click="emit('refresh')">
        <template #icon><icon-refresh /></template>
        重新读取
      </a-button>
    </div>
    <div v-if="targetKinds.length" class="available-models-compare-target">
      <span>目标 Agent</span>
      <a-select
        v-if="targetKinds.length > 1"
        :model-value="preferredKind || undefined"
        size="mini"
        class="available-models-compare-target-select"
        aria-label="选择目标 Agent"
        @change="selectTargetKind"
      >
        <a-option v-for="kind in targetKinds" :key="kind" :value="kind">
          {{ agentCliLabel(store.cliEnvironmentProbe, kind) }}
        </a-option>
      </a-select>
      <strong v-else>{{ activeKindLabel }}</strong>
      <span class="available-models-compare-target-model"><code>{{ model }}</code></span>
    </div>
    <p class="available-models-compare-note">
      价格按各站当前分组和专属倍率换算；官方参考价不乘倍率。没有站点价目的中转站只显示公开参考价，不参与最低价比较。
      <template v-if="targetKinds.length">切换会先把所选模型记为该 Agent 的绑定模型；启动只影响这一次临时 CLI。OAuth 账号只交给本家 Agent。</template>
    </p>
    <p v-if="loading && !rows.length" class="available-models-compare-state" role="status">
      正在读取各站模型资料…
    </p>
    <a-alert v-if="error && !rows.length" type="error" show-icon>
      {{ error }}
    </a-alert>
    <a-spin :loading="loading">
      <div v-if="rows.length" class="available-models-compare-list">
        <article
          v-for="row in rows"
          :key="row.providerId"
          class="available-models-compare-card"
          :class="{ 'is-current': row.current, 'is-lowest': row.lowest }"
        >
          <header class="available-models-compare-card-head">
            <div class="available-models-compare-provider">
              <strong :title="row.label">{{ row.label }}</strong>
              <span v-if="row.current" class="available-models-compare-chip is-current">当前</span>
              <span v-if="row.disabled" class="available-models-compare-chip is-warn">已停用</span>
              <span v-if="row.lowest" class="available-models-compare-chip is-good">最低价</span>
              <span
                v-for="label in servingLabels(row.providerId)"
                :key="label"
                class="available-models-compare-chip is-serving"
                :title="`${label} 的默认配置当前指向此站`"
              >{{ label }} 在用</span>
            </div>
            <span class="available-models-compare-source">{{ row.sourceLabel }}</span>
          </header>
          <div class="available-models-compare-price" :title="row.priceTitle">
            <span v-if="row.price" class="available-models-compare-amount">{{ row.price }}</span>
            <span v-else class="available-models-compare-missing">暂无价格</span>
            <span
              v-if="row.comparison"
              class="available-models-compare-badge"
              :class="`is-${row.comparison.tone}`"
              :title="row.comparison.detail"
            >{{ row.comparison.label }}</span>
          </div>
          <p v-if="row.detail" class="available-models-compare-detail">{{ row.detail }}</p>
          <div class="available-models-compare-facts">
            <span v-if="row.group">分组 {{ row.group }}</span>
            <span v-if="row.quota">{{ row.quota }}</span>
            <span v-if="row.performance">{{ row.performance }}</span>
            <span v-if="row.scopeNote" :title="row.scopeNote">{{ row.scopeNote }}</span>
            <span v-if="row.error" class="is-bad" :title="row.error">资料读取失败：{{ row.error }}</span>
          </div>
          <footer v-if="targetKinds.length" class="available-models-compare-actions">
            <span
              v-if="switchBlockedReason(row) && launchBlockedReason(row)"
              class="available-models-compare-blocked"
            >{{ switchBlockedReason(row) }}</span>
            <a-button
              v-if="row.canSwitch"
              size="mini"
              :type="row.lowest && !row.current ? 'primary' : 'outline'"
              :disabled="Boolean(switchBlockedReason(row))"
              :title="switchBlockedReason(row) || `把 ${model} 写入 ${kindLabel(switchKindFor(row))} 默认配置`"
              @click="switchTo(row)"
            >切换到此站</a-button>
            <a-button
              v-if="row.canLaunch"
              size="mini"
              :disabled="Boolean(launchBlockedReason(row))"
              :title="launchBlockedReason(row) || `用 ${model} 启动 ${kindLabel(launchKindFor(row))} 临时 CLI`"
              @click="launchTo(row)"
            >启动</a-button>
          </footer>
        </article>
      </div>
      <div v-else-if="!loading" class="available-models-empty">
        <span>{{ error || "没有可比的中转站" }}</span>
        <a-button type="text" @click="emit('back')">返回模型列表</a-button>
      </div>
    </a-spin>
  </div>
</template>
