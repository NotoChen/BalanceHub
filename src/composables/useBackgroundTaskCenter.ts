import { Message } from "@arco-design/web-vue";
import { cancelProviderBrowserLogin, listProviderBrowserLogins, showProviderLoginWindow } from "../api/provider-browser-login";
import { createLoginTaskControl } from "../utils/login-task-control";
import { computed, onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "../api/transport.ts";
import type {
  Provider,
  SiteAnnouncementSourceError,
} from "../stores/providers";
import type { TemporaryCliLaunchTask } from "./useWorkspaceLaunchFlow";
import type {
  ProviderBatchOperation,
  ProviderBatchProgressItem,
} from "../api/batch-operation";
import { providerDisplayLabel } from "../utils/provider-display";
import type { CheckInTask } from "../api/checkin";
import { checkInResumeLabel } from "../utils/check-in-tasks";
import type { BrowserRuntimeStatus } from "../api/browser-runtime";
import { supportsCommand } from "./useHostCapabilities";

export type BackgroundTaskStatus = "running" | "waiting" | "success" | "failed" | "cancelled" | "unconfirmed";

export type BackgroundTaskKind =
  | "providerLogin"
  | "cloudSync"
  | "refresh"
  | "checkIn"
  | "announcement"
  | "update"
  | "sync"
  | "cliProbe"
  | "cliLaunch"
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
  /// 需要用户处理或确认：置顶显示并计入顶部徽标。
  attention?: boolean;
  /// 自动维护类任务：只在面板里汇总成一行，不占徽标，也不拦截退出。
  ambient?: boolean;
  actions?: { label: string; disabled?: boolean; run: () => void }[];
}

interface BackgroundTaskEvent {
  taskId: string;
  canCancel?: boolean;
  canShowWindow?: boolean;
  loginAccountId?: string;
  providerId?: string;
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
  openLoginAccount: (id: string) => void;
  openProviderCredentials: (id: string) => void;
  batchOperation: Ref<ProviderBatchOperation | null>;
  batchOperationRunning: Ref<boolean>;
  batchOperationItems: Ref<ProviderBatchProgressItem[]>;
  /// 批次已受理、正在等待自动刷新释放闸门时的说明。
  batchOperationWaitingReason?: Ref<string>;
  batchOperationError: Ref<string>;
  batchOperationCompleted: Ref<boolean>;
  /// 批次正在执行取消；任务中心据此禁用重复点击。
  batchOperationCanceling?: Ref<boolean>;
  /// 取消尚未开始的站点；批次排队或卡住时任务中心也能直接结束它。
  cancelBatchRefresh?: () => Promise<void>;
  refreshInProgress: Ref<boolean>;
  refreshingProviderIds: Ref<Set<string>>;
  checkInTasks: Ref<CheckInTask[]>;
  checkInPending: Ref<string[]>;
  resumeCheckInTask: (task: CheckInTask) => Promise<void>;
  cancelCheckInTask: (task: CheckInTask) => Promise<void>;
  showCheckInWindow: (task: CheckInTask) => Promise<void>;
  browserRuntime: Ref<BrowserRuntimeStatus | null>;
  cancelBrowserRuntime: () => Promise<void>;
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
  temporaryCliLaunchTasks: Ref<TemporaryCliLaunchTask[]>;
  probingCapabilitiesProviderId: Ref<string | null>;
  /// 运行期存储保护态：保存已暂停，只能重启恢复。
  storageBlockedError?: Ref<string | null>;
  restartApp?: () => void;
  domainTasks?: Ref<BackgroundTask[]>;
}

const TASK_EVENT_NAME = "background-task";
const RECENT_TASK_LIMIT = 20;
/// 普通结果只是回看用，短窗口即可，避免面板变成日志。
const RECENT_TASK_MAX_AGE_MS = 15 * 60 * 1_000;
/// 需要用户处理或确认的记录跨会话保留一天，用户回来时还能接着处理。
const RECENT_ATTENTION_MAX_AGE_MS = 24 * 60 * 60 * 1_000;
/// 自动刷新、公告读取、CLI 探测和自动测活属于随时可放弃的维护动作，
/// 卡片和设置里已经有各自的状态。它们只在面板里汇总成一行，失败时才
/// 进入最近记录；不要把徽标和任务列表变成后台轮询日志。
const AMBIENT_KINDS = new Set<BackgroundTaskKind>([
  "autoRefresh",
  "autoLiveness",
  "announcement",
  "cliProbe",
]);
function isAmbientTask(task: Pick<BackgroundTask, "id" | "kind">): boolean {
  // 单卡刷新由卡片自己的 syncing 状态承担，不必再占一条后台任务。
  return task.id === "provider-refresh" || AMBIENT_KINDS.has(task.kind);
}

function isPendingDomainTask(task: BackgroundTask): boolean {
  return task.status === "running"
    || task.status === "waiting"
    || (task.status === "unconfirmed" && task.finishedAt === undefined);
}

/**
 * A single presentation model for work that can otherwise look like a frozen
 * panel.  Business operations remain in their existing stores/composables;
 * this composable only observes them and exposes a compact status stream.
 */
export function useBackgroundTaskCenter(options: UseBackgroundTaskCenterOptions) {
  const remoteTasks = ref<Record<string, BackgroundTask>>({});
  const recentTasks = ref<BackgroundTask[]>([]);
  let schedulerUnlisten: UnlistenFn | null = null;
  let openTaskCenterUnlisten: UnlistenFn | null = null;
  let reconnectUnlisten: UnlistenFn | null = null;
  let recentPruneTimer: number | null = null;
  let previousActive = new Map<string, BackgroundTask>();
  let observedActiveState = false;
  const rememberedTemporaryCliTaskIds = new Set<string>();
  const rememberedCheckInIds = new Set<string>();
  const rememberedDomainTaskResults = new Map<string, string>();
  let disposed = false;
  /// 登录任务最近一次事件序号：列表恢复在请求发出后如果又收到事件，
  /// 事件比列表快照更新，不能用列表覆盖。
  const observedLoginEvents = new Map<string, number>();
  let loginEventSequence = 0;
  const loginControl = createLoginTaskControl({
    changed: (id, pending) => {
      const task = remoteTasks.value[id];
      if (task) remoteTasks.value = { ...remoteTasks.value, [id]: { ...task, actions: task.actions?.map((action) => ({ ...action, disabled: pending })) } };
    },
    failed: (message) => { Message.error(message); },
  });

  function checkInBackgroundTask(task: CheckInTask): BackgroundTask {
    const busy = options.checkInPending.value.some((key) => key.endsWith(`:${task.runId}`));
    const actions: NonNullable<BackgroundTask["actions"]> = [];
    if (task.canShowWindow) actions.push({ label: "显示签到窗口", disabled: busy, run: () => { void options.showCheckInWindow(task); } });
    if (task.canResume) actions.push({ label: checkInResumeLabel(task), disabled: busy, run: () => { void options.resumeCheckInTask(task); } });
    if (task.canCancel) actions.push({ label: "取消", disabled: busy, run: () => { void options.cancelCheckInTask(task); } });
    const status: BackgroundTaskStatus = task.phase === "completed" ? "success" : task.phase === "failed" ? "failed"
      : task.phase === "cancelled" ? "cancelled" : task.phase === "unconfirmed" ? "unconfirmed"
      : task.canResume || ["waitingHuman", "waitingLogin", "waitingBrowser"].includes(task.phase) ? "waiting" : "running";
    return { id: task.runId, kind: task.source === "automatic" ? "autoCheckIn" : "checkIn", title: `签到 · ${task.providerName}`,
      detail: task.message, status, progress: null, startedAt: task.startedAt, finishedAt: task.finishedAt ?? undefined,
      source: task.source === "automatic" ? "automatic" : "manual", actions };
  }

  const activeTasks = computed<BackgroundTask[]>(() => {
    const tasks: BackgroundTask[] = [];
    const now = Date.now();
    const batchItems = options.batchOperationItems.value;

    if (options.batchOperationRunning.value) {
      const completed = batchItems.filter((item) =>
        ["success", "failed", "skipped"].includes(item.status),
      ).length;
      const total = batchItems.length;
      const waitingReason = options.batchOperationWaitingReason?.value ?? "";
      tasks.push({
        id: "manual-batch-operation",
        kind: "refresh",
        title: "全局刷新",
        detail: waitingReason || (total > 0 ? `已完成 ${completed} / ${total} 个中转站` : "正在准备中转站任务"),
        status: "running",
        progress: total > 0 ? completed / total : null,
        startedAt: previousActive.get("manual-batch-operation")?.startedAt ?? now,
        source: "manual",
        actions: options.cancelBatchRefresh
          ? [{
              label: "取消刷新",
              disabled: options.batchOperationCanceling?.value ?? false,
              run: () => { void options.cancelBatchRefresh?.(); },
            }]
          : undefined,
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

    tasks.push(...options.checkInTasks.value.filter((task) => !task.finished).map(checkInBackgroundTask));
    const runtime = options.browserRuntime.value;
    if (runtime && ["installing", "checking"].includes(runtime.phase)) tasks.push({
      id: "browser-runtime-install", kind: "update", title: runtime.phase === "checking" ? "验证浏览器" : "安装浏览器辅助组件", detail: runtime.message,
      status: "running", progress: runtime.progress, startedAt: previousActive.get("browser-runtime-install")?.startedAt ?? now,
      source: "manual", actions: [{ label: "取消", run: () => { void options.cancelBrowserRuntime(); } }],
    });

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
        detail: provider ? `正在探测“${providerDisplayLabel(provider)}”` : "正在探测站点能力",
        status: "running",
        progress: null,
        startedAt: previousActive.get("capability-probe")?.startedAt ?? now,
        source: "manual",
      });
    }

    for (const task of options.temporaryCliLaunchTasks.value) {
      if (task.status === "running") {
        tasks.push(temporaryCliBackgroundTask(task));
      }
    }

    const domainTasks = options.domainTasks?.value ?? [];
    const domainTaskIds = new Set(domainTasks.map((task) => task.id));
    for (const task of Object.values(remoteTasks.value)) {
      // Domain snapshots recover missed events and own the final state for the same ID.
      if ((task.status === "running" || task.status === "waiting") && !domainTaskIds.has(task.id)) tasks.push(task);
    }
    for (const task of domainTasks) {
      if (isPendingDomainTask(task)) tasks.push(task);
    }

    // 运行期存储保护不属于任何一次请求：卡片仍在，但所有保存都已暂停，
    // 用户只有重启应用才能恢复。放进任务中心可确保无论当前在哪个页面
    // 都能看到这条状态并直接重启。
    const storageBlocked = options.storageBlockedError?.value;
    if (storageBlocked) {
      tasks.push({
        id: "storage-blocked",
        kind: "sync",
        title: "本地配置已暂停写入",
        detail: storageBlocked,
        status: "failed",
        progress: null,
        startedAt: previousActive.get("storage-blocked")?.startedAt ?? now,
        source: "automatic",
        attention: true,
        actions: options.restartApp
          ? [{ label: "重启应用", run: options.restartApp }]
          : undefined,
      });
    }
    return tasks
      .map((task) => ({
        ...task,
        ambient: task.ambient ?? isAmbientTask(task),
        attention:
          task.attention ??
          (task.status === "waiting" || task.status === "unconfirmed"),
      }))
      .sort((left, right) => left.startedAt - right.startedAt);
  });

  /// 徽标只统计用户真正需要看到的任务；自动维护在面板里单独提示。
  const activeTaskCount = computed(
    () => activeTasks.value.filter((task) => !task.ambient).length,
  );
  /// 需要处理的任务可能停在进行中，也可能是已经结束、但还留着操作入口的记录。
  const attentionTaskCount = computed(() => {
    const ids = new Set<string>();
    for (const task of activeTasks.value) if (task.attention) ids.add(task.id);
    for (const task of recentTasks.value) if (task.attention) ids.add(task.id);
    return ids.size;
  });
  const ambientTaskCount = computed(
    () => activeTasks.value.filter((task) => task.ambient).length,
  );

  function pruneRecent() {
    const now = Date.now();
    recentTasks.value = recentTasks.value
      .filter((task) => {
        const maxAge = task.attention
          ? RECENT_ATTENTION_MAX_AGE_MS
          : RECENT_TASK_MAX_AGE_MS;
        return (task.finishedAt ?? 0) >= now - maxAge;
      })
      .slice(0, RECENT_TASK_LIMIT);
  }

  function rememberRecent(task: BackgroundTask) {
    const finished: BackgroundTask = {
      ...task,
      status: task.status === "running" ? "success" : task.status,
      finishedAt: task.finishedAt ?? Date.now(),
    };
    finished.ambient = finished.ambient ?? isAmbientTask(finished);
    finished.attention =
      finished.attention ??
      (finished.status === "unconfirmed"
        || (finished.status === "failed" && Boolean(finished.actions?.length)));
    // 自动维护成功时不留记录，否则面板会被每轮自动刷新、公告读取和 CLI
    // 探测刷屏；失败仍要留下，方便回看是哪一项维护动作出的问题。
    if (finished.ambient && finished.status !== "failed") return;
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
        if (
          !next.has(id)
          && previous.kind !== "providerLogin"
          && previous.kind !== "cloudSync"
          && !id.startsWith("scheduler-")
          && !id.startsWith("temporary-cli-launch-")
          && !id.startsWith("checkin-")
          && id !== "browser-runtime-install"
          && id !== "storage-blocked"
          && !id.startsWith("agent-")
        ) {
          rememberRecent(completedLocalTask(previous));
        }
      }
    }
    previousActive = next;
    observedActiveState = true;
  }

  function handleSchedulerEvent(event: BackgroundTaskEvent) {
    if (!event || !event.taskId || !event.title) return;
    if (options.domainTasks?.value.some((task) => task.id === event.taskId)) return;
    const actions: NonNullable<BackgroundTask["actions"]> = [];
    if (event.kind === "providerLogin") {
      const disabled = loginControl.pending(event.taskId);
      if (event.canShowWindow) actions.push({ label: "显示登录窗口", disabled, run: () => { void loginControl.run(event.taskId, () => showProviderLoginWindow(event.taskId)); } });
      if (event.canCancel) actions.push({ label: "取消", disabled, run: () => { void loginControl.run(event.taskId, () => cancelProviderBrowserLogin(event.taskId)); } });
      if (event.status === "success") {
        const providerId = event.providerId;
        const accountId = event.loginAccountId;
        if (providerId) actions.push({ label: "查看站点凭据", run: () => options.openProviderCredentials(providerId) });
        else if (accountId) actions.push({ label: "查看登录账号", run: () => options.openLoginAccount(accountId) });
      } else if (event.status === "failed" && event.providerId) {
        // 自动重新认证失败时用户能做的只有重新走一次可见登录；把入口放在
        // 失败任务本身，不让它只留下一条错误文案。
        const providerId = event.providerId;
        actions.push({ label: "打开站点凭据", run: () => options.openProviderCredentials(providerId) });
      }
    }
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
      source: event.kind === "providerLogin" ? "manual" : "automatic",
      actions,
    };
    if (event.status === "running" || event.status === "waiting") {
      remoteTasks.value = { ...remoteTasks.value, [task.id]: task };
      return;
    }
    const next = { ...remoteTasks.value };
    delete next[task.id];
    remoteTasks.value = next;
    rememberRecent(task);
  }

  /// 登录任务只在启动和事件流重连时用后端列表校准：事件断开期间结束的
  /// 任务不会再有终态事件，不重新读取就会一直停在运行中。
  async function recoverProviderLogins() {
    if (disposed || !supportsCommand("list_provider_browser_logins")) return;
    const trackedBefore = new Set(
      Object.values(remoteTasks.value)
        .filter((task) => task.kind === "providerLogin")
        .map((task) => task.id),
    );
    const sequenceBefore = loginEventSequence;
    try {
      const tasks = await listProviderBrowserLogins();
      if (disposed) return;
      const listed = new Set(tasks.map((task) => task.runId));
      for (const task of tasks) {
        // 请求在途时收到的事件比这份列表新，保留事件结果。
        if ((observedLoginEvents.get(task.runId) ?? 0) > sequenceBefore) continue;
        handleSchedulerEvent({
          taskId: task.runId, kind: "providerLogin",
          title: `${task.operation === "account" ? "管理登录账号" : task.operation === "reauth" ? "重新认证" : "登录并导入"} · ${task.providerName}`,
          detail: task.message,
          status: task.phase === "completed" ? "success" : task.phase === "failed" ? "failed"
            : task.phase === "cancelled" ? "cancelled" : task.phase === "queued" || task.phase === "waitingLogin" ? "waiting" : "running",
          progress: null, startedAt: task.startedAt, finishedAt: task.finishedAt, error: task.error,
          canCancel: task.canCancel, canShowWindow: task.canShowWindow,
          loginAccountId: task.loginAccountId, providerId: task.providerId ?? undefined,
        });
      }
      // 请求前仍在跟踪、列表里却没有的任务，后端已不再跟踪（服务端重启
      // 或记录被清理），不会再有终态事件。标成待确认，避免一直转圈。
      for (const id of trackedBefore) {
        if (listed.has(id)) continue;
        if ((observedLoginEvents.get(id) ?? 0) > sequenceBefore) continue;
        const task = remoteTasks.value[id];
        if (!task) continue;
        const next = { ...remoteTasks.value };
        delete next[id];
        remoteTasks.value = next;
        rememberRecent({
          ...task,
          status: "unconfirmed",
          detail: "登录任务已从后台任务列表消失（应用可能已重启），请重新发起登录",
          finishedAt: Date.now(),
        });
      }
    } catch {
      // 恢复是尽力而为；下一次重连或事件会再试。
    }
  }

  function handleTemporaryCliTaskChanges(tasks: TemporaryCliLaunchTask[]) {
    for (const task of tasks) {
      if (task.status === "running" || rememberedTemporaryCliTaskIds.has(task.id)) {
        continue;
      }
      rememberedTemporaryCliTaskIds.add(task.id);
      rememberRecent(temporaryCliBackgroundTask(task));
    }

    const retainedIds = new Set([
      ...tasks.map((task) => task.id),
      ...recentTasks.value.map((task) => task.id),
    ]);
    for (const id of rememberedTemporaryCliTaskIds) {
      if (!retainedIds.has(id)) {
        rememberedTemporaryCliTaskIds.delete(id);
      }
    }
  }

  onMounted(async () => {
    pruneRecent();
    recentPruneTimer = window.setInterval(pruneRecent, 60_000);
    try {
      const dispose = await listen<BackgroundTaskEvent>(TASK_EVENT_NAME, (event) => {
        if (event.payload.kind === "providerLogin") {
          observedLoginEvents.set(event.payload.taskId, ++loginEventSequence);
        }
        handleSchedulerEvent(event.payload);
      });
      if (disposed) dispose();
      else schedulerUnlisten = dispose;
      // 桌面托盘「后台任务」：窗口被唤到前台后打开任务面板。单独兜底，
      // 不让一个可选入口影响下面的登录任务恢复。
      try {
        const openDispose = await listen("open-task-center", () => {
          if (!disposed) window.dispatchEvent(new CustomEvent("balancehub-open-task-center"));
        });
        if (disposed) openDispose();
        else openTaskCenterUnlisten = openDispose;
      } catch {
        // 旧版桌面或浏览器预览没有这个入口。
      }
      const reconnectDispose = await listen("balancehub-events-reconnected", () => {
        if (!disposed) void recoverProviderLogins();
      });
      if (disposed) reconnectDispose();
      else reconnectUnlisten = reconnectDispose;
      await recoverProviderLogins();
    } catch {
      // Vite/browser preview has no Tauri event bus; local observable tasks remain available.
    }
  });

  onUnmounted(() => {
    disposed = true;
    loginControl.dispose();
    schedulerUnlisten?.();
    schedulerUnlisten = null;
    openTaskCenterUnlisten?.();
    openTaskCenterUnlisten = null;
    reconnectUnlisten?.();
    reconnectUnlisten = null;
    if (recentPruneTimer !== null) {
      window.clearInterval(recentPruneTimer);
      recentPruneTimer = null;
    }
  });

  watch(activeTasks, handleActiveTaskChanges, { deep: true, immediate: true });
  watch(options.temporaryCliLaunchTasks, handleTemporaryCliTaskChanges, {
    deep: true,
    immediate: true,
  });
  watch(options.checkInTasks, (tasks) => {
    for (const task of tasks) {
      if (task.finished && !rememberedCheckInIds.has(task.runId)) {
        rememberedCheckInIds.add(task.runId);
        rememberRecent(checkInBackgroundTask(task));
      }
    }
  }, { immediate: true });
  watch(options.browserRuntime, (runtime, previous) => {
    if (!runtime || !previous || !["installing", "checking"].includes(previous.phase) || ["installing", "checking"].includes(runtime.phase)) return;
    rememberRecent({ id: "browser-runtime-install", kind: "update", title: previous.phase === "checking" ? "验证浏览器" : "浏览器辅助组件", detail: runtime.message,
      status: runtime.phase === "ready" ? "success" : runtime.phase === "cancelled" ? "cancelled" : "failed",
      progress: null, startedAt: previousActive.get("browser-runtime-install")?.startedAt ?? Date.now(), finishedAt: Date.now(), source: "manual" });
  });
  if (options.domainTasks) watch(options.domainTasks, (tasks) => {
    const retained = new Set(tasks.map((task) => task.id));
    if (Object.keys(remoteTasks.value).some((id) => retained.has(id))) {
      remoteTasks.value = Object.fromEntries(Object.entries(remoteTasks.value).filter(([id]) => !retained.has(id)));
    }
    const pendingIds = new Set(tasks.filter(isPendingDomainTask).map((task) => task.id));
    if (pendingIds.size) recentTasks.value = recentTasks.value.filter((task) => !pendingIds.has(task.id));
    for (const task of tasks) {
      if (isPendingDomainTask(task)) { rememberedDomainTaskResults.delete(task.id); continue; }
      const signature = `${task.status}:${task.finishedAt}:${task.detail}`;
      if (rememberedDomainTaskResults.get(task.id) === signature) continue;
      rememberedDomainTaskResults.set(task.id, signature);
      rememberRecent(task);
    }
    for (const id of rememberedDomainTaskResults.keys()) if (!retained.has(id)) rememberedDomainTaskResults.delete(id);
  }, { immediate: true });

  return {
    activeTasks,
    activeTaskCount,
    attentionTaskCount,
    ambientTaskCount,
    recentTasks,
    clearRecentTasks,
  };
}

function temporaryCliBackgroundTask(task: TemporaryCliLaunchTask): BackgroundTask {
  return {
    id: task.id,
    kind: "cliLaunch",
    title: task.title,
    detail: task.detail,
    status: task.status,
    progress: null,
    startedAt: task.startedAt,
    finishedAt: task.finishedAt,
    error: task.error,
    source: "manual",
  };
}
