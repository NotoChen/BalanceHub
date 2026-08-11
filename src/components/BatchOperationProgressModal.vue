<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Ban,
  CalendarCheck2,
  CheckCircle2,
  CircleAlert,
  CircleDashed,
  LoaderCircle,
  RefreshCw,
} from "@lucide/vue";
import type {
  ProviderBatchOperation,
  ProviderBatchProgressItem,
  ProviderBatchStatus,
} from "../api/batch-operation";
import { formatQuotaValue } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  operation: ProviderBatchOperation | null;
  running: boolean;
  items: ProviderBatchProgressItem[];
  error: string;
  startedAt: number | null;
  finishedAt: number | null;
  completed: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
}>();

type RowFilter = "all" | ProviderBatchStatus;
const rowFilter = ref<RowFilter>("all");

const title = computed(() =>
  props.operation === "checkIn" ? "一键签到进度" : "全局刷新进度",
);
const icon = computed(() => (props.operation === "checkIn" ? CalendarCheck2 : RefreshCw));
const total = computed(() => props.items.length);
const completedCount = computed(
  () =>
    props.items.filter((item) =>
      ["success", "failed", "skipped"].includes(item.status),
    ).length,
);
const successCount = computed(() => count("success"));
const failedCount = computed(() => count("failed"));
const skippedCount = computed(() => count("skipped"));
const runningCount = computed(() => count("running"));
// Arco Progress expects a ratio between 0 and 1, not a percentage between 0 and 100.
const percent = computed(() => {
  if (!total.value) return props.completed ? 1 : 0;
  return Math.min(1, Math.max(0, completedCount.value / total.value));
});
const rows = computed(() =>
  rowFilter.value === "all"
    ? props.items
    : props.items.filter((item) => item.status === rowFilter.value),
);

function count(status: ProviderBatchStatus) {
  return props.items.filter((item) => item.status === status).length;
}

function statusLabel(status: ProviderBatchStatus) {
  return {
    pending: "等待",
    running: "处理中",
    success: "成功",
    failed: "失败",
    skipped: "已跳过",
  }[status];
}

function statusIcon(status: ProviderBatchStatus) {
  return {
    pending: CircleDashed,
    running: LoaderCircle,
    success: CheckCircle2,
    failed: CircleAlert,
    skipped: Ban,
  }[status];
}

function quotaLabel(value: number, item: ProviderBatchProgressItem) {
  const details = item.details;
  if (!details) return "-";
  return formatQuotaValue(value, {
    quotaDisplayType: details.quotaDisplayType || "currency",
    currencySymbol: details.currencySymbol || "$",
  });
}

function formatTime(value: string | null | undefined) {
  if (!value) return "-";
  const raw = Number(value);
  const date = Number.isFinite(raw)
    ? new Date(raw < 1_000_000_000_000 ? raw * 1000 : raw)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}

function durationLabel() {
  if (!props.startedAt) return "";
  const end = props.finishedAt ?? Date.now();
  const duration = Math.max(0, end - props.startedAt);
  return duration < 1000 ? duration + " ms" : (duration / 1000).toFixed(1) + " s";
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal batch-operation-modal"
    :footer="false"
    :width="820"
    :unmount-on-close="false"
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title batch-operation-title">
        <span class="surface-modal-title-icon"><component :is="icon" :size="18" :stroke-width="1.9" /></span>
        <span class="surface-modal-title-copy"><strong>{{ title }}</strong></span>
        <span class="surface-modal-title-meta" :class="{ running, completed: completed && !running }">
          {{ running ? "正在执行" : completed ? "已完成" : "等待执行" }}
        </span>
      </div>
    </template>

    <div class="batch-operation-panel">
      <section class="batch-operation-summary" :class="{ 'is-running': running, 'has-error': error }">
        <div class="batch-operation-summary-main">
          <span class="batch-operation-summary-dot" aria-hidden="true" />
          <div>
            <strong>{{ running ? "后端正在逐站处理" : completed ? "批量操作已完成" : "批量操作尚未开始" }}</strong>
            <span v-if="error">{{ error }}</span>
            <span v-else>{{ completedCount }} / {{ total }} 个中转站已结束 · 用时 {{ durationLabel() || "-" }}</span>
          </div>
        </div>
        <div class="batch-operation-summary-counts">
          <span class="is-success">成功 {{ successCount }}</span>
          <span class="is-failed">失败 {{ failedCount }}</span>
          <span class="is-skipped">跳过 {{ skippedCount }}</span>
        </div>
      </section>

      <div class="batch-operation-progress-line">
        <a-progress :percent="percent" :show-text="false" :status="error ? 'danger' : completed ? 'success' : 'normal'" />
        <strong>{{ Math.round(percent * 100) }}%</strong>
      </div>

      <div class="batch-operation-filters" role="tablist" aria-label="批量结果筛选">
        <button type="button" :class="{ active: rowFilter === 'all' }" @click="rowFilter = 'all'">全部 {{ total }}</button>
        <button type="button" :class="{ active: rowFilter === 'running' }" @click="rowFilter = 'running'">处理中 {{ runningCount }}</button>
        <button type="button" :class="{ active: rowFilter === 'success' }" @click="rowFilter = 'success'">成功 {{ successCount }}</button>
        <button type="button" :class="{ active: rowFilter === 'failed' }" @click="rowFilter = 'failed'">失败 {{ failedCount }}</button>
        <button type="button" :class="{ active: rowFilter === 'skipped' }" @click="rowFilter = 'skipped'">跳过 {{ skippedCount }}</button>
      </div>

      <div class="batch-operation-rows">
        <div v-if="rows.length === 0" class="batch-operation-empty">暂无匹配的中转站</div>
        <article
          v-for="item in rows"
          :key="item.providerId"
          class="batch-operation-row"
          :class="'is-' + item.status"
        >
          <div class="batch-operation-row-heading">
            <span class="batch-operation-row-status">
              <component :is="statusIcon(item.status)" :class="{ spinning: item.status === 'running' }" :size="16" :stroke-width="2" />
              <strong>{{ statusLabel(item.status) }}</strong>
            </span>
            <strong class="batch-operation-row-name">{{ item.name || item.baseUrl }}</strong>
            <span v-if="item.details?.lastSyncedAt" class="batch-operation-row-time">同步 {{ formatTime(item.details.lastSyncedAt) }}</span>
            <span v-if="item.details?.lastCheckedInAt" class="batch-operation-row-time">签到 {{ formatTime(item.details.lastCheckedInAt) }}</span>
          </div>
          <p v-if="item.message" class="batch-operation-row-message">{{ item.message }}</p>
          <div v-if="item.details && item.status === 'success'" class="batch-operation-row-details">
            <span>可用 {{ item.details.unlimited ? "无限" : item.details.known ? quotaLabel(item.details.available, item) : "未知" }}</span>
            <span>已用 {{ item.details.known ? quotaLabel(item.details.used, item) : "未知" }}</span>
            <span v-if="item.details.modelCount > 0">模型 {{ item.details.modelCount }}</span>
            <span v-if="item.details.userId">用户 ID {{ item.details.userId }}</span>
            <span v-if="item.details.username">{{ item.details.username }}</span>
            <span v-if="item.details.quotaDelta !== null && item.details.quotaDelta !== undefined" class="is-reward">
              奖励 +{{ quotaLabel(item.details.quotaDelta, item) }}
            </span>
          </div>
        </article>
      </div>

      <footer class="batch-operation-footer">
        <span v-if="running" class="batch-operation-footer-hint">可以关闭窗口，任务会继续在后台执行</span>
        <a-button @click="emit('update:visible', false)">关闭</a-button>
      </footer>
    </div>
  </a-modal>
</template>
