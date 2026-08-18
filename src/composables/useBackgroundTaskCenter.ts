import { computed, onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Provider,
  SiteAnnouncementSourceError,
} from "../stores/providers";
import type {
  ProviderBatchOperation,
  ProviderBatchProgressItem,
} from "../api/batch-operation";

export type BackgroundTaskStatus = "running" | "success" | "failed";

export type BackgroundTaskKind =
  | "refresh"
  | "checkIn"
  | "announcement"
  | "update"
  | "sync"
  | "cliProbe"
  | "autoRefresh"
  | "autoCheckIn"
  | "autoLiveness";

export interface BackgroundTask {
  id: string;
  kind: BackgroundTaskKind;
  title: string;
  detail: string;
  status: BackgroundTaskStatus;
  progress: number | null;
  startedAt: number;
  finishedAt?: number;
  error?: string;
  source: "manual" | "automatic";
}

interface BackgroundTaskEvent {
  taskId: string;
  kind: BackgroundTaskKind;
  status: BackgroundTaskStatus;
  title: string;
  detail: string;
  progress: number | null;
  startedAt: number;
  finishedAt?: number | null;
  error?: string | null;
}

interface UseBackgroundTaskCenterOptions {
  providers: Ref<Provider[]>;
  batchOperation: Ref<ProviderBatchOperation | null>;
  batchOperationRunning: Ref<boolean>;
  batchOperationItems: Ref<ProviderBatchProgressItem[]>;
  batchOperationError: Ref<string>;
  batchOperationCompleted: Ref<boolean>;
  refreshInProgress: Ref<boolean>;
  refreshingProviderIds: Ref<Set<string>>;
  globalCheckInInProgress: Ref<boolean>;
  checkingInProviderIds: Ref<string[]>;
  checkingForUpdate: Ref<boolean>;
  updateCheckError: Ref<string>;
  installingUpdate: Ref<boolean>;
  updateDownloadProgress: Ref<number | null>;
  updateInstallStatus: Ref<string>;
  updateInstallError: Ref<string>;
  announcementsLoading: Ref<boolean>;
  announcementFatalError: Ref<string>;
  announcementErrors: Ref<SiteAnnouncementSourceError[]>;
  cliRuntimeLoading: Ref<boolean>;
  probingCapabilitiesProviderId: Ref<string | null>;
}

const TASK_EVENT_NAME = "background-task";
const RECENT_TASK_LIMIT = 12;
const RECENT_TASK_MAX_AGE_MS = 15 * 60 * 1_000;

/**
 * A single presentation model for work that can otherwise look like a frozen
 * panel.  Business operations remain in their existing stores/composables;
 * this composable only observes them and exposes a compact status stream.
 */
export function useBackgroundTaskCenter(options: UseBackgroundTaskCenterOptions) {
  const remoteTasks = ref<Record<string, BackgroundTask>>({});
  const recentTasks = ref<BackgroundTask[]>([]);
  let schedulerUnlisten: UnlistenFn | null = null;
  let recentPruneTimer: number | null = null;
  let previousActive = new Map<string, BackgroundTask>();
  let observedActiveState = false;

  const activeTasks = computed<BackgroundTask[]>(() => {
    const tasks: BackgroundTask[] = [];
    const now = Date.now();
    const batchItems = options.batchOperationItems.value;

    if (options.batchOperationRunning.value) {
      const operation = options.batchOperation.value;
      const completed = batchItems.filter((item) =>
        ["success", "failed", "skipped"].includes(item.status),
      ).length;
      const total = batchItems.length;
      tasks.push({
        id: "manual-batch-operation",
        kind: operation === "checkIn" ? "checkIn" : "refresh",
        title: operation === "checkIn" ? "一键签到" : "全局刷新",
        detail: total > 0 ? `已完成 ${completed} / ${total} 个中转站` : "正在准备中转站任务",
        status: "running",
        progress: total > 0 ? completed / total : null,
        startedAt: previousActive.get("manual-batch-operation")?.startedAt ?? now,
        source: "manual",
      });
    }

    if (
      (options.refreshInProgress.value || options.refreshingProviderIds.value.size > 0) &&
      !options.batchOperationRunning.value
    ) {
      const syncing = Math.max(
        options.refreshingProviderIds.value.size,
        options.providers.value.filter((provider) => provider.runtime.status === "syncing").length,
      );
      const hasAutomaticRefresh = Object.values(remoteTasks.value).some(
        (task) => task.status === "running" && task.kind === "autoRefresh",
      );
      if (!hasAutomaticRefresh) {
        tasks.push({
          id: "provider-refresh",
          kind: "sync",
          title: "同步中转站",
          detail: syncing > 0 ? `${syncing} 个中转站正在同步` : "正在读取最新数据",
          status: "running",
          progress: null,
          startedAt: previousActive.get("provider-refresh")?.startedAt ?? now,
          source: "automatic",
        });
      }
    }

    if (options.globalCheckInInProgress.value && !options.batchOperationRunning.value) {
      tasks.push({
        id: "provider-check-in",
        kind: "checkIn",
        title: "签到中转站",
        detail: "正在处理签到请求",
        status: "running",
        progress: null,
        startedAt: previousActive.get("provider-check-in")?.startedAt ?? now,
        source: "manual",
      });
    } else if (options.checkingInProviderIds.value.length > 0 && !options.batchOperationRunning.value) {
      tasks.push({
        id: "provider-check-in",
        kind: "checkIn",
        title: "签到中转站",
        detail: `${options.checkingInProviderIds.value.length} 个中转站正在签到`,
        status: "running",
        progress: null,
        startedAt: previousActive.get("provider-check-in")?.startedAt ?? now,
        source: "manual",
      });
    }

    if (options.checkingForUpdate.value) {
      tasks.push({
        id: "update-check",
        kind: "update",
        title: "检查应用更新",
        detail: "正在查询最新版本",
        status: "running",
        progress: null,
        startedAt: previousActive.get("update-check")?.startedAt ?? now,
        source: "automatic",
      });
    }

    if (options.installingUpdate.value) {
      const rawProgress = options.updateDownloadProgress.value;
      tasks.push({
        id: "update-install",
        kind: "update",
        title: "安装应用更新",
        detail: options.updateInstallStatus.value || "正在准备更新",
        status: "running",
        progress: rawProgress === null ? null : Math.max(0, Math.min(1, rawProgress / 100)),
        startedAt: previousActive.get("update-install")?.startedAt ?? now,
        source: "manual",
      });
    }

    if (options.announcementsLoading.value) {
      tasks.push({
        id: "site-announcements",
        kind: "announcement",
        title: "读取站点公告",
        detail: "正在并发读取已启用站点",
        status: "running",
        progress: null,
        startedAt: previousActive.get("site-announcements")?.startedAt ?? now,
        source: "automatic",
      });
    }

    if (options.cliRuntimeLoading.value) {
      tasks.push({
        id: "cli-runtime-probe",
        kind: "cliProbe",
        title: "检测 Agent CLI",
        detail: "正在读取本机 CLI 与活动实例",
        status: "running",
        progress: null,
        startedAt: previousActive.get("cli-runtime-probe")?.startedAt ?? now,
        source: "automatic",
      });
    }

    if (options.probingCapabilitiesProviderId.value) {
      const provider = options.providers.value.find(
        (candidate) => candidate.identity.id === options.probingCapabilitiesProviderId.value,
      );
      tasks.push({
        id: "capability-probe",
        kind: "sync",
        title: "探测站点能力",
        detail: provider ? `正在探测“${provider.identity.name}”` : "正在探测站点能力",
        status: "running",
        progress: null,
        startedAt: previousActive.get("capability-probe")?.startedAt ?? now,
        source: "manual",
      });
    }

    for (const task of Object.values(remoteTasks.value)) {
      if (task.status === "running") tasks.push(task);
    }
    return tasks.sort((left, right) => left.startedAt - right.startedAt);
  });

  const activeTaskCount = computed(() => activeTasks.value.length);

  function pruneRecent() {
    const cutoff = Date.now() - RECENT_TASK_MAX_AGE_MS;
    recentTasks.value = recentTasks.value
      .filter((task) => (task.finishedAt ?? 0) >= cutoff)
      .slice(0, RECENT_TASK_LIMIT);
  }

  function rememberRecent(task: BackgroundTask) {
    const finished = {
      ...task,
      status: task.status === "running" ? "success" : task.status,
      finishedAt: task.finishedAt ?? Date.now(),
    } satisfies BackgroundTask;
    recentTasks.value = [
      finished,
      ...recentTasks.value.filter((candidate) => candidate.id !== finished.id),
    ].slice(0, RECENT_TASK_LIMIT);
    pruneRecent();
  }

  function clearRecentTasks() {
    recentTasks.value = [];
  }

  function completedLocalTask(task: BackgroundTask): BackgroundTask {
    let failureMessage = "";
    if (
      task.id === "manual-batch-operation" &&
      (!options.batchOperationCompleted.value || options.batchOperationError.value)
    ) {
      failureMessage = options.batchOperationError.value || "批量任务未完整结束";
    } else if (task.id === "update-check") {
      failureMessage = options.updateCheckError.value;
    } else if (task.id === "update-install") {
      failureMessage = options.updateInstallError.value;
    } else if (task.id === "site-announcements") {
      failureMessage =
        options.announcementFatalError.value ||
        (options.announcementErrors.value.length > 0
          ? `${options.announcementErrors.value.length} 个站点公告读取失败`
          : "");
    }
    const failed = Boolean(failureMessage);
    const successDetail = (() => {
      if (task.id === "manual-batch-operation") {
        const succeeded = options.batchOperationItems.value.filter(
          (item) => item.status === "success",
        ).length;
        const failedItems = options.batchOperationItems.value.filter(
          (item) => item.status === "failed",
        ).length;
        const skipped = options.batchOperationItems.value.filter(
          (item) => item.status === "skipped",
        ).length;
        return `成功 ${succeeded} · 失败 ${failedItems} · 跳过 ${skipped}`;
      }
      switch (task.id) {
        case "provider-refresh":
          return "中转站同步已完成";
        case "provider-check-in":
          return "签到任务已完成";
        case "update-check":
          return "版本检查已完成";
        case "update-install":
          return options.updateInstallStatus.value || "应用更新已完成";
        case "site-announcements":
          return "站点公告读取已完成";
        case "cli-runtime-probe":
          return "Agent CLI 检测已完成";
        case "capability-probe":
          return "站点能力探测已完成";
        default:
          return task.detail;
      }
    })();
    return {
      ...task,
      status: failed ? "failed" : "success",
      detail: failed ? failureMessage : successDetail,
      error: failed ? failureMessage : undefined,
      finishedAt: Date.now(),
    };
  }

  function handleActiveTaskChanges(current: BackgroundTask[]) {
    const next = new Map(current.map((task) => [task.id, task]));
    if (observedActiveState) {
      for (const [id, previous] of previousActive) {
        if (!next.has(id) && !id.startsWith("scheduler-")) {
          rememberRecent(completedLocalTask(previous));
        }
      }
    }
    previousActive = next;
    observedActiveState = true;
  }

  function handleSchedulerEvent(event: BackgroundTaskEvent) {
    if (!event || !event.taskId || !event.title) return;
    const task: BackgroundTask = {
      id: event.taskId,
      kind: event.kind,
      title: event.title,
      detail: event.detail,
      status: event.status,
      progress: event.progress ?? null,
      startedAt: event.startedAt || Date.now(),
      finishedAt: event.finishedAt ?? undefined,
      error: event.error ?? undefined,
      source: "automatic",
    };
    if (event.status === "running") {
      remoteTasks.value = { ...remoteTasks.value, [task.id]: task };
      return;
    }
    const next = { ...remoteTasks.value };
    delete next[task.id];
    remoteTasks.value = next;
    rememberRecent(task);
  }

  onMounted(async () => {
    pruneRecent();
    recentPruneTimer = window.setInterval(pruneRecent, 60_000);
    try {
      schedulerUnlisten = await listen<BackgroundTaskEvent>(TASK_EVENT_NAME, (event) => {
        handleSchedulerEvent(event.payload);
      });
    } catch {
      // Vite/browser preview has no Tauri event bus; local observable tasks remain available.
    }
  });

  onUnmounted(() => {
    schedulerUnlisten?.();
    schedulerUnlisten = null;
    if (recentPruneTimer !== null) {
      window.clearInterval(recentPruneTimer);
      recentPruneTimer = null;
    }
  });

  watch(activeTasks, handleActiveTaskChanges, { deep: true, immediate: true });

  return {
    activeTasks,
    activeTaskCount,
    recentTasks,
    clearRecentTasks,
  };
}
