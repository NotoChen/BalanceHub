<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, useId } from "vue";
import { Activity, CalendarCheck2, CloudDownload, LogIn, Megaphone, RefreshCw, Search, Terminal } from "@lucide/vue";
import type { BackgroundTask, BackgroundTaskKind } from "../composables/useBackgroundTaskCenter";
import { formatProgress } from "../utils/progress-display";

const props = defineProps<{
  tasks: BackgroundTask[];
  recentTasks: BackgroundTask[];
  activeCount: number;
  attentionCount: number;
  ambientCount: number;
}>();

const gradientId = useId();

const emit = defineEmits<{
  clearRecent: [];
}>();

const popupVisible = ref(false);

/// 托盘菜单和系统级入口通过这个 DOM 事件请求打开任务面板；组件本身不直接
/// 依赖 Tauri 事件总线，SSR/浏览器预览下也不会重复注册。
function openFromHost() {
  popupVisible.value = true;
}
onMounted(() => window.addEventListener("balancehub-open-task-center", openFromHost));
onUnmounted(() => window.removeEventListener("balancehub-open-task-center", openFromHost));

/// 需要处理的任务可能停在进行中，也可能是已经结束、但还留着操作入口的记录。
/// 同一个 id 只展示一次，以进行中的那条为准。
const attentionTasks = computed(() => {
  const rows: BackgroundTask[] = [];
  const seen = new Set<string>();
  for (const task of props.tasks) {
    if (!task.attention) continue;
    seen.add(task.id);
    rows.push(task);
  }
  for (const task of props.recentTasks) {
    if (!task.attention || seen.has(task.id)) continue;
    seen.add(task.id);
    rows.push(task);
  }
  return rows;
});

const runningTasks = computed(
  () => props.tasks.filter((task) => !task.attention && !task.ambient),
);

const historyTasks = computed(() => {
  const attentionIds = new Set(attentionTasks.value.map((task) => task.id));
  return props.recentTasks.filter((task) => !attentionIds.has(task.id));
});

interface TaskRow {
  task: BackgroundTask;
  meta: string;
  /// 历史记录里的操作通常要跳转到对应页面；点完就收起面板。
  closesPopup: boolean;
}

interface TaskGroup {
  key: "attention" | "active" | "recent";
  title: string;
  rows: TaskRow[];
}

const groups = computed<TaskGroup[]>(() => {
  const result: TaskGroup[] = [];
  if (attentionTasks.value.length > 0) {
    result.push({
      key: "attention",
      title: "需要处理",
      rows: attentionTasks.value.map((task) => ({
        task,
        meta: task.finishedAt
          ? `${statusLabel(task)} · ${formatTime(task.finishedAt)}`
          : statusLabel(task),
        closesPopup: Boolean(task.finishedAt),
      })),
    });
  }
  if (runningTasks.value.length > 0) {
    result.push({
      key: "active",
      title: "进行中",
      rows: runningTasks.value.map((task) => ({
        task,
        meta: statusLabel(task),
        closesPopup: false,
      })),
    });
  }
  if (historyTasks.value.length > 0) {
    result.push({
      key: "recent",
      title: "最近完成",
      rows: historyTasks.value.map((task) => ({
        task,
        meta: formatTime(task.finishedAt),
        closesPopup: true,
      })),
    });
  }
  return result;
});

const hasRunningTasks = computed(() =>
  props.tasks.some((task) => task.status === "running" || task.status === "waiting"),
);
const hasAttention = computed(() => props.attentionCount > 0);

const panelSummary = computed(() => {
  if (hasAttention.value) {
    return runningTasks.value.length > 0
      ? `${props.attentionCount} 项需要处理 · ${runningTasks.value.length} 项进行中`
      : `${props.attentionCount} 项需要处理`;
  }
  if (runningTasks.value.length > 0) return `${runningTasks.value.length} 项进行中`;
  if (props.recentTasks.length > 0) return `${props.recentTasks.length} 项最近任务`;
  if (props.ambientCount > 0) return `${props.ambientCount} 项自动维护进行中`;
  return "当前没有进行中的任务";
});

function iconFor(kind: BackgroundTaskKind) {
  switch (kind) {
    case "providerLogin":
      return LogIn;
    case "refresh":
    case "sync":
    case "autoRefresh":
      return RefreshCw;
    case "checkIn":
    case "autoCheckIn":
      return CalendarCheck2;
    case "announcement":
      return Megaphone;
    case "update":
    case "cloudSync":
      return CloudDownload;
    case "cliProbe":
    case "cliLaunch":
      return Terminal;
    case "autoLiveness":
      return Activity;
    default:
      return Search;
  }
}

function statusLabel(task: BackgroundTask) {
  if (task.status === "failed") return "失败";
  if (task.status === "success") return "完成";
  if (task.status === "waiting") return "待处理";
  if (task.status === "cancelled") return "已取消";
  if (task.status === "unconfirmed") return "待确认";
  if (task.progress === null) return "进行中";
  return formatProgress(task.progress);
}

function formatTime(value?: number) {
  if (!value) return "刚刚";
  const date = new Date(value);
  const now = new Date();
  const clock = `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  const startOfDay = (target: Date) =>
    new Date(target.getFullYear(), target.getMonth(), target.getDate()).getTime();
  const days = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
  if (days <= 0) return clock;
  if (days === 1) return `昨天 ${clock}`;
  return `${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")} ${clock}`;
}

function openResult(run: () => void) {
  popupVisible.value = false;
  run();
}
</script>

<template>
  <a-popover
    v-model:popup-visible="popupVisible"
    trigger="click"
    position="br"
    content-class="background-task-popover"
  >
    <button
      type="button"
      class="topbar-task-button"
      :class="{ 'has-active': hasRunningTasks, 'has-attention': hasAttention }"
      :title="hasAttention || tasks.length > 0 || recentTasks.length > 0 ? panelSummary : '后台任务'"
      :aria-busy="hasRunningTasks"
      aria-label="查看后台任务"
    >
      <Activity
        class="topbar-task-icon"
        :color="hasRunningTasks ? `url(#${gradientId})` : undefined"
        :size="18"
        :stroke-width="1.9"
        aria-hidden="true"
      >
        <defs v-if="hasRunningTasks">
          <linearGradient
            :id="gradientId"
            gradientUnits="userSpaceOnUse"
            x1="1"
            y1="3"
            x2="23"
            y2="21"
          >
            <stop class="background-task-gradient-stop is-first" offset="0%" stop-color="#165dff" />
            <stop class="background-task-gradient-stop is-second" offset="34%" stop-color="#32c6d4" />
            <stop class="background-task-gradient-stop is-third" offset="68%" stop-color="#7a5af8" />
            <stop class="background-task-gradient-stop is-fourth" offset="100%" stop-color="#165dff" />
          </linearGradient>
        </defs>
      </Activity>
      <span
        v-if="hasAttention"
        class="topbar-action-badge topbar-task-badge is-attention"
      >
        {{ attentionCount > 99 ? "99+" : attentionCount }}
      </span>
      <span
        v-else-if="activeCount > 0"
        class="topbar-action-badge topbar-task-badge"
      >
        {{ activeCount > 99 ? "99+" : activeCount }}
      </span>
      <span
        v-else-if="recentTasks.length > 0"
        class="topbar-task-result-dot"
        aria-hidden="true"
      />
    </button>

    <template #content>
      <section class="background-task-panel" aria-label="后台任务">
        <header class="background-task-panel-header">
          <div class="background-task-heading">
            <strong>后台任务</strong>
            <span>{{ panelSummary }}</span>
          </div>
          <div class="background-task-header-actions">
            <button
              v-if="recentTasks.length > 0"
              type="button"
              class="background-task-clear"
              @click.stop="emit('clearRecent')"
            >
              清空记录
            </button>
            <span class="background-task-panel-pulse" :class="{ active: hasRunningTasks }" aria-hidden="true" />
          </div>
        </header>

        <div
          v-for="group in groups"
          :key="group.key"
          class="background-task-section"
          :class="`is-${group.key}`"
          :aria-label="group.title"
        >
          <div class="background-task-section-title">
            <span>{{ group.title }}</span>
            <em>{{ group.rows.length }}</em>
          </div>
          <article
            v-for="row in group.rows"
            :key="`${group.key}-${row.task.id}-${row.task.finishedAt ?? 0}`"
            class="background-task-row"
            :class="`is-${row.task.status}`"
          >
            <span class="background-task-row-icon">
              <component :is="iconFor(row.task.kind)" :size="16" :stroke-width="1.9" />
            </span>
            <div class="background-task-row-copy">
              <strong>{{ row.task.title }}</strong>
              <span>{{ row.task.error || row.task.detail }}</span>
              <div v-if="row.task.actions?.length" class="background-task-actions">
                <button
                  v-for="(action, index) in row.task.actions"
                  :key="`${row.task.id}-${index}`"
                  type="button"
                  :disabled="action.disabled"
                  @click.stop="row.closesPopup ? openResult(action.run) : action.run()"
                >{{ action.label }}</button>
              </div>
              <a-progress
                v-if="row.task.progress !== null && row.task.status === 'running'"
                :percent="row.task.progress"
                :show-text="false"
                size="small"
                animation
              />
            </div>
            <em>{{ row.meta }}</em>
          </article>
        </div>

        <div v-if="ambientCount > 0" class="background-task-ambient">
          <Activity :size="14" :stroke-width="1.9" aria-hidden="true" />
          <span>自动维护 · {{ ambientCount }} 项进行中</span>
        </div>

        <div
          v-if="groups.length === 0 && recentTasks.length === 0 && ambientCount === 0"
          class="background-task-empty"
        >
          <Activity :size="22" :stroke-width="1.6" aria-hidden="true" />
          <span>后台任务会在这里显示进度和结果</span>
        </div>
      </section>
    </template>
  </a-popover>
</template>

<style scoped>
.background-task-actions { display: flex; gap: 12px; margin-top: 5px; }
.background-task-actions button { padding: 2px 0; border: 0; background: transparent; color: rgb(var(--primary-6)); font-size: 12px; cursor: pointer; }
.background-task-actions button:disabled { opacity: 0.5; cursor: default; }
</style>
