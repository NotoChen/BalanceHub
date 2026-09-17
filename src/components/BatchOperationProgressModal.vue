<script setup lang="ts">
import { computed, ref, watch } from "vue";
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
import type { CheckInTask } from "../api/checkin";
import { formatQuotaValue } from "../utils/provider-display";

const props = withDefaults(defineProps<{
  visible: boolean;
  operation: ProviderBatchOperation | "checkIn" | null;
  running: boolean;
  items: ProviderBatchProgressItem[];
  error: string;
  startedAt: number | null;
  finishedAt: number | null;
  completed: boolean;
  checkInTasks?: CheckInTask[];
  checkInPending?: string[];
  skippedCount?: number | null;
  submitting?: boolean;
}>(), {
  checkInTasks: () => [],
  checkInPending: () => [],
  skippedCount: 0,
  submitting: false,
});

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  resumeCheckIn: [task: CheckInTask];
  cancelCheckIn: [task: CheckInTask];
}>();

type RowStatus = ProviderBatchStatus | "waiting" | "unconfirmed" | "cancelled";
type RowFilter = "all" | RowStatus;
interface ProgressRow extends Omit<ProviderBatchProgressItem, "status"> {
  status: RowStatus;
  task?: CheckInTask;
}
const rowFilter = ref<RowFilter>("all");
watch(() => props.startedAt, () => { rowFilter.value = "all"; });

const isCheckIn = computed(() => props.operation === "checkIn");
const title = computed(() => isCheckIn.value ? "一键签到进度" : "全局刷新进度");
const icon = computed(() => isCheckIn.value ? CalendarCheck2 : RefreshCw);
const progressItems = computed<ProgressRow[]>(() => isCheckIn.value
  ? props.checkInTasks.map((task) => ({
      providerId: task.providerId, name: task.providerName, baseUrl: "",
      status: checkInStatus(task), message: task.message, task,
    }))
  : props.items);
const extraSkipped = computed(() => isCheckIn.value ? props.skippedCount ?? 0 : 0);
const total = computed(() => progressItems.value.length + extraSkipped.value);
const completedCount = computed(
  () => progressItems.value.filter((item) => item.task
    ? item.task.finished
    : ["success", "failed", "skipped"].includes(item.status)).length + extraSkipped.value,
);
const successCount = computed(() => count("success"));
const failedCount = computed(() => count("failed"));
const skippedTotal = computed(() => count("skipped") + extraSkipped.value);
const runningCount = computed(() => count("running"));
const waitingCount = computed(() => count("waiting"));
const unconfirmedCount = computed(() => count("unconfirmed"));
const cancelledCount = computed(() => count("cancelled"));
const waitingOnly = computed(() => props.running && waitingCount.value > 0
  && !runningCount.value && !count("pending"));
const hasIssues = computed(() => Boolean(props.error) || failedCount.value > 0 || unconfirmedCount.value > 0);
const progressStatus = computed(() => props.error || failedCount.value > 0 ? "danger"
  : props.completed && !unconfirmedCount.value && !cancelledCount.value ? "success" : "normal");
const summaryTitle = computed(() => {
  if (!isCheckIn.value) return props.running ? "后端正在逐站处理" : props.completed ? "批量操作已完成" : "批量操作尚未开始";
  if (props.submitting) return "正在创建签到任务";
  if (waitingOnly.value) return "等待处理验证，可在下方继续";
  if (props.running) return "正在逐站签到";
  if (props.error) return "签到任务提交失败";
  if (props.completed && !progressItems.value.length) return "当前没有需要签到的中转站";
  if (props.completed) return hasIssues.value ? "签到任务已结束，请查看各站点结果" : "一键签到已结束";
  return "正在读取签到任务";
});
// Arco Progress expects a ratio between 0 and 1, not a percentage between 0 and 100.
const percent = computed(() => {
  if (!total.value) return props.completed ? 1 : 0;
  return Math.min(1, Math.max(0, completedCount.value / total.value));
});
const rows = computed(() =>
  rowFilter.value === "all"
    ? progressItems.value
    : progressItems.value.filter((item) => item.status === rowFilter.value),
);

function count(status: RowStatus) {
  return progressItems.value.filter((item) => item.status === status).length;
}

function checkInStatus(task: CheckInTask): RowStatus {
  if (!task.finished) {
    if (task.canResume || task.phase === "waitingHuman" || task.phase === "waitingBrowser") return "waiting";
    return task.phase === "queued" ? "pending" : "running";
  }
  if (task.phase === "completed") return "success";
  if (task.phase === "cancelled") return "cancelled";
  if (task.phase === "unconfirmed") return "unconfirmed";
  return "failed";
}

function statusLabel(item: ProgressRow) {
  if (item.task?.phase === "waitingBrowser") return "等待组件";
  return {
    pending: "等待",
    running: "处理中",
    success: "成功",
    failed: "失败",
    skipped: "已跳过",
    waiting: "等待验证",
    unconfirmed: "结果待确认",
    cancelled: "已取消",
  }[item.status];
}

function statusIcon(status: RowStatus) {
  return {
    pending: CircleDashed,
    running: LoaderCircle,
    success: CheckCircle2,
    failed: CircleAlert,
    skipped: Ban,
    waiting: CircleDashed,
    unconfirmed: CircleAlert,
    cancelled: Ban,
  }[status];
}

function taskActionPending(task: CheckInTask) {
  return props.checkInPending.includes(`resume:${task.runId}`)
    || props.checkInPending.includes(`cancel:${task.runId}`);
}

function quotaLabel(value: number, item: ProgressRow) {
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
        <span class="surface-modal-title-meta" :class="{ running, completed: completed && !running && !hasIssues && !cancelledCount }">
          {{ submitting ? "正在提交" : waitingOnly ? "等待处理" : running ? "正在执行" : error ? "提交失败" : completed ? "已结束" : "等待执行" }}
        </span>
      </div>
    </template>

    <div class="batch-operation-panel">
      <section class="batch-operation-summary" :class="{ 'is-running': running && !waitingOnly, 'has-error': hasIssues }" aria-live="polite">
        <div class="batch-operation-summary-main">
          <span class="batch-operation-summary-dot" aria-hidden="true" />
          <div>
            <strong>{{ summaryTitle }}</strong>
            <span v-if="error">{{ error }}</span>
            <span v-else>{{ completedCount }} / {{ total }} 个中转站已结束 · 用时 {{ durationLabel() || "-" }}</span>
          </div>
        </div>
        <div class="batch-operation-summary-counts">
          <span class="is-success">成功 {{ successCount }}</span>
          <span class="is-failed">失败 {{ failedCount }}</span>
          <span v-if="!isCheckIn || props.skippedCount !== null" class="is-skipped">跳过 {{ skippedTotal }}</span>
          <span v-if="waitingCount" class="is-waiting">待处理 {{ waitingCount }}</span>
          <span v-if="unconfirmedCount" class="is-unconfirmed">待确认 {{ unconfirmedCount }}</span>
          <span v-if="cancelledCount" class="is-cancelled">取消 {{ cancelledCount }}</span>
        </div>
      </section>

      <div class="batch-operation-progress-line">
        <a-progress :percent="percent" :show-text="false" :status="progressStatus" />
        <strong>{{ Math.round(percent * 100) }}%</strong>
      </div>

      <div class="batch-operation-filters" role="tablist" aria-label="批量结果筛选">
        <button type="button" :class="{ active: rowFilter === 'all' }" @click="rowFilter = 'all'">全部 {{ total }}</button>
        <button type="button" :class="{ active: rowFilter === 'running' }" @click="rowFilter = 'running'">处理中 {{ runningCount }}</button>
        <button v-if="waitingCount" type="button" :class="{ active: rowFilter === 'waiting' }" @click="rowFilter = 'waiting'">待处理 {{ waitingCount }}</button>
        <button type="button" :class="{ active: rowFilter === 'success' }" @click="rowFilter = 'success'">成功 {{ successCount }}</button>
        <button type="button" :class="{ active: rowFilter === 'failed' }" @click="rowFilter = 'failed'">失败 {{ failedCount }}</button>
        <button v-if="!isCheckIn || props.skippedCount !== null" type="button" :class="{ active: rowFilter === 'skipped' }" @click="rowFilter = 'skipped'">跳过 {{ skippedTotal }}</button>
        <button v-if="unconfirmedCount" type="button" :class="{ active: rowFilter === 'unconfirmed' }" @click="rowFilter = 'unconfirmed'">待确认 {{ unconfirmedCount }}</button>
        <button v-if="cancelledCount" type="button" :class="{ active: rowFilter === 'cancelled' }" @click="rowFilter = 'cancelled'">取消 {{ cancelledCount }}</button>
      </div>

      <div class="batch-operation-rows">
        <div v-if="rows.length === 0" class="batch-operation-empty">
          {{ submitting ? "正在准备签到任务…" : extraSkipped > 0 && (rowFilter === 'all' || rowFilter === 'skipped') ? `已跳过 ${extraSkipped} 个当前无需签到的中转站` : "暂无匹配的中转站" }}
        </div>
        <article
          v-for="item in rows"
          :key="item.task?.runId ?? item.providerId"
          class="batch-operation-row"
          :class="'is-' + item.status"
        >
          <div class="batch-operation-row-heading">
            <span class="batch-operation-row-status">
              <component :is="statusIcon(item.status)" :class="{ spinning: item.status === 'running' }" :size="16" :stroke-width="2" />
              <strong>{{ statusLabel(item) }}</strong>
            </span>
            <strong class="batch-operation-row-name">{{ item.name || item.baseUrl }}</strong>
            <span v-if="item.details?.lastSyncedAt" class="batch-operation-row-time">同步 {{ formatTime(item.details.lastSyncedAt) }}</span>
            <span v-if="item.details?.lastCheckedInAt" class="batch-operation-row-time">签到 {{ formatTime(item.details.lastCheckedInAt) }}</span>
          </div>
          <p v-if="item.message" class="batch-operation-row-message">{{ item.message }}</p>
          <div v-if="item.task && (item.task.canResume || item.task.canCancel)" class="batch-operation-row-actions">
            <a-button v-if="item.task.canResume" size="small" type="primary" :disabled="taskActionPending(item.task)" @click="emit('resumeCheckIn', item.task)">继续签到</a-button>
            <a-button v-if="item.task.canCancel" size="small" :disabled="taskActionPending(item.task)" @click="emit('cancelCheckIn', item.task)">取消</a-button>
          </div>
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
