import { computed, onMounted, onUnmounted, provide, shallowRef, watch, type ComputedRef, type InjectionKey } from "vue";
import { Message } from "@arco-design/web-vue";
import { checkInProvider, checkInAllProviders, cancelCheckInTask, listCheckInTasks, listenCheckInTasks, resumeCheckInTask, showCheckInWindow, type CheckInTask } from "../api/checkin";
import { listen, type UnlistenFn } from "../api/transport.ts";
import type { Provider } from "../stores/providers";
import { createCheckInTracker, type CheckInSnapshot } from "../utils/check-in-tasks";
import type { BrowserRuntimeController } from "./useBrowserRuntime";
import { useCheckInBatchProgress } from "./useCheckInBatchProgress";

interface CheckInController {
  tasks: ComputedRef<CheckInTask[]>;
  pending: ComputedRef<string[]>;
  resume: (task: CheckInTask) => Promise<void>;
  cancel: (task: CheckInTask) => Promise<void>;
  showWindow: (task: CheckInTask) => Promise<void>;
}
export const CHECK_IN_CONTEXT: InjectionKey<CheckInController> = Symbol("check-in-tasks");

export function useCheckInActions(options: {
  /// 任务结束后要失效的缓存和重读的数据；providerIds 是本次完成的站点。
  reload: (providerIds: string[]) => Promise<unknown>;
  browserRuntime: BrowserRuntimeController;
}) {
  const state = shallowRef<CheckInSnapshot>({ items: [], pending: [], error: "" });
  const tracker = createCheckInTracker({ list: listCheckInTasks, listen: listenCheckInTasks,
    submit: checkInProvider, submitAll: checkInAllProviders, resume: resumeCheckInTask, cancel: cancelCheckInTask,
    showWindow: showCheckInWindow,
  }, (snapshot) => { state.value = snapshot; });
  const tasks = computed(() => state.value.items);
  const pending = computed(() => state.value.pending);
  const batchProgress = useCheckInBatchProgress({ snapshot: () => state.value, submit: () => tracker.submitAll() });
  const checkingInProviderIds = computed(() => [...new Set([
    ...tasks.value.filter((task) => !task.finished && !task.canResume).map((task) => task.providerId),
    ...pending.value.filter((key) => key.startsWith("submit:")).map((key) => key.slice(7)),
  ])]);
  const globalCheckInInProgress = computed(() => batchProgress.progress.value.running || pending.value.includes("batch")
    || tasks.value.some((task) => task.source === "batch" && !task.finished));

  async function resume(task: CheckInTask) {
    if (task.phase === "waitingBrowser" && !options.browserRuntime.state.value?.ready) {
      options.browserRuntime.open();
      return;
    }
    await tracker.resume(task.runId);
  }
  async function cancel(task: CheckInTask) { await tracker.cancel(task.runId); }
  async function showWindow(task: CheckInTask) { await tracker.showWindow(task.runId); }

  async function checkInProviderAction(provider: Provider) {
    const existing = tasks.value.find((task) => task.providerId === provider.identity.id && !task.finished);
    if (existing) {
      if (existing.canResume) { await resume(existing); return; }
      // 批量签到留下的验证窗口可能被其他窗口盖住；再点「签到」就把已有
      // 窗口带到前面，而不是再发一次没有任何反馈的提交。
      if (existing.canShowWindow) { await showWindow(existing); return; }
      Message.info(`该站点正在签到：${existing.message}`);
      return;
    }
    const task = await tracker.submit(provider.identity.id);
    if (task?.canResume) { await resume(task); return; }
    if (task?.canShowWindow) await showWindow(task);
    // 同一登录账号被多张卡片共享时，后端会复用已有任务，返回的 providerId
    // 可能不是当前卡片；本地任务列表还没收到事件时也会走到这里。两种情况都
    // 必须给出说明，不能让「签到」看起来点了没反应。
    if (task && !task.finished && !task.canShowWindow) {
      Message.info(
        task.providerId === provider.identity.id
          ? `该站点正在签到：${task.message}`
          : `该登录账号正在「${task.providerName}」签到：${task.message}`,
      );
    }
  }

  async function checkInAllProvidersAction() {
    await batchProgress.open();
  }

  const seen = new Set<string>();
  const prompted = new Set<string>();
  let historySeeded = false;
  let disposed = false;
  let reconnectUnlisten: UnlistenFn | null = null;
  watch(tasks, (items) => {
    // 第一份后端任务全集里包含前端重载前就已结束的历史签到；把这些
    // 任务记为已见，避免每次刷新页面都把历史结果重新弹一遍。
    if (!historySeeded) {
      if (!state.value.listed) return;
      historySeeded = true;
      for (const task of items) {
        if (task.finished) {
          seen.add(task.runId);
        } else if (task.phase === "waitingBrowser" && task.source === "manual" && !prompted.has(task.runId)) {
          prompted.add(task.runId);
          options.browserRuntime.open();
        }
      }
      return;
    }
    const finishedProviderIds = new Set<string>();
    for (const task of items) {
      if (task.phase === "waitingBrowser" && task.source === "manual" && !prompted.has(task.runId)) {
        prompted.add(task.runId);
        options.browserRuntime.open();
      }
      if (task.finished && !seen.has(task.runId)) {
        seen.add(task.runId);
        finishedProviderIds.add(task.providerId);
        if (task.source === "manual") {
          if (task.phase === "completed") Message.success(task.message);
          else if (task.phase !== "cancelled") Message.warning(task.message);
        }
      }
    }
    if (finishedProviderIds.size > 0) {
      void options.reload([...finishedProviderIds]).catch(() => {});
    }
  });
  watch(() => state.value.error, (error) => { if (error) Message.error(error); });
  onMounted(() => {
    void tracker.start();
    // 服务端事件流重连后，断开期间结束的签到任务不会再有终态事件；
    // 重新读取后端任务全集，任务中心和批量进度才能退出“运行中”。
    void listen("balancehub-events-reconnected", () => {
      if (!disposed) void tracker.refresh();
    }).then((stop) => {
      if (disposed) stop();
      else reconnectUnlisten = stop;
    }).catch(() => {
      // 桌面版没有这条补偿事件。
    });
    window.addEventListener("focus", tracker.refresh);
  });
  onUnmounted(() => {
    disposed = true;
    reconnectUnlisten?.();
    reconnectUnlisten = null;
    tracker.stop();
    window.removeEventListener("focus", tracker.refresh);
  });
  provide(CHECK_IN_CONTEXT, { tasks, pending, resume, cancel, showWindow });
  return { checkInTasks: tasks, checkInPending: pending, resumeCheckInTask: resume, cancelCheckInTask: cancel,
    showCheckInWindow: showWindow,
    checkInBatchVisible: batchProgress.visible, checkInBatchProgress: batchProgress.progress,
    checkingInProviderIds, globalCheckInInProgress, checkInProviderAction, checkInAllProvidersAction };
}
