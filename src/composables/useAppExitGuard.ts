import { onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { isWebHost, listen, type UnlistenFn } from "../api/transport.ts";
import { quitApp, setAppRunningTasks } from "../api/app";
import type { BackgroundTask } from "./useBackgroundTaskCenter";
import { exitBlockingTasks } from "../utils/app-exit";

interface UseAppExitGuardOptions {
  activeTasks: Ref<BackgroundTask[]>;
  /// 设置仍在防抖、保存中或保存失败时也值得拦一次退出：关掉进程就再没有
  /// 机会补写，用户刚改的设置会静默丢失。
  pendingSettings?: Ref<boolean>;
  /// 关窗前先尝试落盘；返回 true 表示队列已经确认保存。
  flushSettings?: () => Promise<boolean>;
}

/// 退出确认由 Rust 的关窗/退出事件触发：Rust 只认识「有没有需要确认的状态」
/// 这一位，具体是任务还是未保存设置、是否真的需要弹窗由前端判断，避免把
/// 任务模型复制到宿主层。
export function useAppExitGuard(options: UseAppExitGuardOptions) {
  const exitConfirmVisible = ref(false);
  const exitConfirmTasks = ref<BackgroundTask[]>([]);
  const exitConfirmSettingsPending = ref(false);
  const settingsPending = options.pendingSettings ?? ref(false);
  let unlisten: UnlistenFn | null = null;
  let disposed = false;
  let reportedRunning: boolean | null = null;
  /// 每次关窗请求的序号。取消或确认后递增，让仍在补写设置的旧请求失效，
  /// 避免用户点了“继续运行”又被旧请求弹窗或直接退出。
  let exitAttempt = 0;

  function blockingTasks() {
    return exitBlockingTasks(options.activeTasks.value);
  }

  function needsExitGuard() {
    return blockingTasks().length > 0 || settingsPending.value;
  }

  async function syncRunning(running: boolean) {
    if (isWebHost) return;
    try {
      await setAppRunningTasks(running);
    } catch {
      // 退出守卫是尽力而为；同步失败不能影响正常操作。
    }
  }

  async function openExitConfirm() {
    // 已经在确认或补写设置时不重复打开；重复关窗只保留最新一次尝试。
    if (exitConfirmVisible.value) return;
    const attempt = ++exitAttempt;
    // 先把防抖中或失败待重试的设置补写一次；成功就不必为它拦人。
    if (settingsPending.value && options.flushSettings) {
      await options.flushSettings();
    }
    // 补写期间用户可能已经取消、再次关窗或组件已卸载。
    if (disposed || attempt !== exitAttempt) return;
    const tasks = blockingTasks();
    const pendingSettings = settingsPending.value;
    // Rust 与前端状态之间可能有一拍延迟；没有需要确认的内容时不要弹空窗。
    if (tasks.length === 0 && !pendingSettings) {
      void confirmExit();
      return;
    }
    exitConfirmTasks.value = tasks;
    exitConfirmSettingsPending.value = pendingSettings;
    exitConfirmVisible.value = true;
  }

  async function confirmExit() {
    exitAttempt++;
    exitConfirmVisible.value = false;
    exitConfirmTasks.value = [];
    try {
      await quitApp();
    } catch (error) {
      Message.error(
        `退出失败：${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }

  function cancelExit() {
    exitAttempt++;
    exitConfirmVisible.value = false;
    exitConfirmTasks.value = [];
    exitConfirmSettingsPending.value = false;
  }

  watch(
    needsExitGuard,
    (running) => {
      if (running === reportedRunning) return;
      reportedRunning = running;
      void syncRunning(running);
    },
    { immediate: true },
  );

  // 弹窗打开期间任务可能自己完成：列表要跟着收缩，全部结束且设置也落盘后
  // 直接关闭，不要继续拿已完成的任务拦住用户。
  watch(
    [() => blockingTasks(), settingsPending],
    ([tasks, pending]) => {
      if (!exitConfirmVisible.value) return;
      exitConfirmSettingsPending.value = pending;
      if (tasks.length === 0 && !pending) {
        cancelExit();
        return;
      }
      exitConfirmTasks.value = tasks;
    },
  );

  onMounted(async () => {
    disposed = false;
    try {
      const stop = await listen("app-close-requested", () => {
        if (!disposed) openExitConfirm();
      });
      if (disposed) stop();
      else unlisten = stop;
    } catch {
      // 浏览器版没有桌面关闭事件。
    }
  });

  onUnmounted(() => {
    disposed = true;
    exitAttempt++;
    unlisten?.();
    unlisten = null;
    // 组件重建后 immediate watch 会重新同步；这里清掉缓存，避免新实例
    // 以为 Rust 侧仍然是 true 而跳过同步。
    reportedRunning = null;
    void syncRunning(false);
  });

  return {
    exitConfirmVisible,
    exitConfirmTasks,
    exitConfirmSettingsPending,
    confirmExit,
    cancelExit,
  };
}
