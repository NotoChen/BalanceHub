import { computed, onMounted, onUnmounted, provide, shallowRef, watch, type ComputedRef, type InjectionKey } from "vue";
import { Message } from "@arco-design/web-vue";
import { checkInProvider, checkInAllProviders, cancelCheckInTask, listCheckInTasks, listenCheckInTasks, resumeCheckInTask, type CheckInTask } from "../api/checkin";
import type { Provider } from "../stores/providers";
import { createCheckInTracker, type CheckInSnapshot } from "../utils/check-in-tasks";
import type { BrowserRuntimeController } from "./useBrowserRuntime";

interface CheckInController {
  tasks: ComputedRef<CheckInTask[]>;
  pending: ComputedRef<string[]>;
  resume: (task: CheckInTask) => Promise<void>;
  cancel: (task: CheckInTask) => Promise<void>;
}
export const CHECK_IN_CONTEXT: InjectionKey<CheckInController> = Symbol("check-in-tasks");

export function useCheckInActions(options: { reload: () => Promise<unknown>; browserRuntime: BrowserRuntimeController }) {
  const state = shallowRef<CheckInSnapshot>({ items: [], pending: [], error: "" });
  const tracker = createCheckInTracker({ list: listCheckInTasks, listen: listenCheckInTasks,
    submit: checkInProvider, submitAll: checkInAllProviders, resume: resumeCheckInTask, cancel: cancelCheckInTask,
  }, (snapshot) => { state.value = snapshot; });
  const tasks = computed(() => state.value.items);
  const pending = computed(() => state.value.pending);
  const checkingInProviderIds = computed(() => [...new Set([
    ...tasks.value.filter((task) => !task.finished && !task.canResume).map((task) => task.providerId),
    ...pending.value.filter((key) => key.startsWith("submit:")).map((key) => key.slice(7)),
  ])]);
  const globalCheckInInProgress = computed(() => pending.value.includes("batch")
    || tasks.value.some((task) => task.source === "batch" && !task.finished && !task.canResume));

  async function resume(task: CheckInTask) {
    if (task.phase === "waitingBrowser" && !options.browserRuntime.state.value?.ready) {
      options.browserRuntime.open();
      return;
    }
    await tracker.resume(task.runId);
  }
  async function cancel(task: CheckInTask) { await tracker.cancel(task.runId); }

  async function checkInProviderAction(provider: Provider) {
    const existing = tasks.value.find((task) => task.providerId === provider.identity.id && !task.finished);
    if (existing?.canResume) { await resume(existing); return; }
    const task = await tracker.submit(provider.identity.id);
    if (task?.canResume) await resume(task);
  }

  async function checkInAllProvidersAction() {
    const batch = await tracker.submitAll();
    if (batch) Message.info(batch.tasks.length ? `${batch.tasks.length} 个签到任务已加入后台，跳过 ${batch.skipped} 个` : "当前没有需要签到的中转站");
  }

  const seen = new Set<string>();
  const prompted = new Set<string>();
  watch(tasks, (items) => {
    let reload = false;
    for (const task of items) {
      if (task.phase === "waitingBrowser" && task.source === "manual" && !prompted.has(task.runId)) {
        prompted.add(task.runId);
        options.browserRuntime.open();
      }
      if (task.finished && !seen.has(task.runId)) {
        seen.add(task.runId);
        reload = true;
        if (task.source === "manual") {
          if (task.phase === "completed") Message.success(task.message);
          else if (task.phase !== "cancelled") Message.warning(task.message);
        }
      }
    }
    if (reload) void options.reload().catch(() => {});
  });
  watch(() => state.value.error, (error) => { if (error) Message.error(error); });
  onMounted(() => { void tracker.start(); window.addEventListener("focus", tracker.refresh); });
  onUnmounted(() => { tracker.stop(); window.removeEventListener("focus", tracker.refresh); });
  provide(CHECK_IN_CONTEXT, { tasks, pending, resume, cancel });
  return { checkInTasks: tasks, checkInPending: pending, resumeCheckInTask: resume, cancelCheckInTask: cancel,
    checkingInProviderIds, globalCheckInInProgress, checkInProviderAction, checkInAllProvidersAction };
}
