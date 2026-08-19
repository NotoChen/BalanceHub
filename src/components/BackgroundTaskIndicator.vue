<script setup lang="ts">
import { ref, watch } from "vue";
import { Activity, CalendarCheck2, CheckCircle2, CircleAlert, CloudDownload, LoaderCircle, Megaphone, RefreshCw, Search, Terminal } from "@lucide/vue";
import type { BackgroundTask, BackgroundTaskKind } from "../composables/useBackgroundTaskCenter";

const props = defineProps<{
  tasks: BackgroundTask[];
  recentTasks: BackgroundTask[];
  activeCount: number;
}>();

const emit = defineEmits<{
  clearRecent: [];
}>();

const popupVisible = ref(false);

watch(popupVisible, (visible, previous) => {
  if (!visible && previous && props.recentTasks.length > 0) {
    emit("clearRecent");
  }
});

function iconFor(kind: BackgroundTaskKind) {
  switch (kind) {
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
  if (task.progress === null) return "进行中";
  return `${Math.round(task.progress * 100)}%`;
}

function formatTime(value?: number) {
  if (!value) return "刚刚";
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(value));
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
      :class="{ 'has-active': activeCount > 0 }"
      :title="activeCount > 0 ? `${activeCount} 项后台任务正在执行` : recentTasks.length > 0 ? `${recentTasks.length} 项任务结果待查看` : '后台任务'"
      aria-label="查看后台任务"
    >
      <LoaderCircle v-if="activeCount > 0" class="topbar-action-spin" :size="18" :stroke-width="1.9" />
      <Activity v-else :size="18" :stroke-width="1.9" />
      <span v-if="activeCount > 0" class="topbar-action-badge topbar-task-badge">
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
            <span>{{ activeCount > 0 ? `${activeCount} 项正在执行` : recentTasks.length > 0 ? `${recentTasks.length} 项结果待查看` : "当前没有运行中的任务" }}</span>
          </div>
          <div class="background-task-header-actions">
            <button
              v-if="recentTasks.length > 0"
              type="button"
              class="background-task-clear"
              @click.stop="emit('clearRecent')"
            >
              清空已完成
            </button>
            <span class="background-task-panel-pulse" :class="{ active: activeCount > 0 }" aria-hidden="true" />
          </div>
        </header>

        <div v-if="tasks.length > 0" class="background-task-list" aria-label="正在执行">
          <article v-for="task in tasks" :key="task.id" class="background-task-row is-running">
            <span class="background-task-row-icon">
              <component :is="iconFor(task.kind)" :size="16" :stroke-width="1.9" />
            </span>
            <div class="background-task-row-copy">
              <strong>{{ task.title }}</strong>
              <span>{{ task.detail }}</span>
              <a-progress
                v-if="task.progress !== null"
                :percent="task.progress"
                :show-text="false"
                size="small"
                animation
              />
            </div>
            <em>{{ statusLabel(task) }}</em>
          </article>
        </div>

        <div v-if="recentTasks.length > 0" class="background-task-recent">
          <div class="background-task-section-title">待查看结果</div>
          <article
            v-for="task in recentTasks.slice(0, 5)"
            :key="`${task.id}-${task.finishedAt}`"
            class="background-task-row"
            :class="`is-${task.status}`"
          >
            <span class="background-task-row-icon">
              <CheckCircle2 v-if="task.status === 'success'" :size="16" :stroke-width="2" />
              <CircleAlert v-else-if="task.status === 'failed'" :size="16" :stroke-width="2" />
              <RefreshCw v-else :size="16" :stroke-width="2" />
            </span>
            <div class="background-task-row-copy">
              <strong>{{ task.title }}</strong>
              <span>{{ task.error || task.detail }}</span>
            </div>
            <time>{{ formatTime(task.finishedAt) }}</time>
          </article>
        </div>

        <div v-if="tasks.length === 0 && recentTasks.length === 0" class="background-task-empty">
          <Activity :size="22" :stroke-width="1.6" aria-hidden="true" />
          <span>后台任务会在这里显示进度和结果</span>
        </div>
      </section>
    </template>
  </a-popover>
</template>
