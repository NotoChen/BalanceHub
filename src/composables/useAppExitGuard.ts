import { onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { isWebHost, listen, type UnlistenFn } from "../api/transport.ts";
import { quitApp, setAppRunningTasks } from "../api/app";
import type { BackgroundTask } from "./useBackgroundTaskCenter";
import { exitBlockingTasks } from "../utils/app-exit";

interface UseAppExitGuardOptions {
  activeTasks: Ref<BackgroundTask[]>;
}

/// 退出确认由 Rust 的关窗/退出事件触发：Rust 只认识「有没有任务」这一位，
/// 具体是哪些任务、是否真的需要拦下来由前端判断，避免把任务模型复制到宿主层。
export function useAppExitGuard(options: UseAppExitGuardOptions) {
  const exitConfirmVisible = ref(false);
  const exitConfirmTasks = ref<BackgroundTask[]>([]);
  let unlisten: UnlistenFn | null = null;
  let disposed = false;
  let reportedRunning: boolean | null = null;

  function blockingTasks() {
    return exitBlockingTasks(options.activeTasks.value);
  }

  async function syncRunning(running: boolean) {
    if (isWebHost) return;
    try {
      await setAppRunningTasks(running);
    } catch {
      // 退出守卫是尽力而为；同步失败不能影响正常操作。
    }
  }

  function openExitConfirm() {
    const tasks = blockingTasks();
    // Rust 与前端状态之间可能有一拍延迟；没有任务时不要弹空窗，直接放行。
    if (tasks.length === 0) {
      void confirmExit();
      return;
    }
    exitConfirmTasks.value = tasks;
    exitConfirmVisible.value = true;
  }

  async function confirmExit() {
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
    exitConfirmVisible.value = false;
    exitConfirmTasks.value = [];
  }

  watch(
    () => blockingTasks().length > 0,
    (running) => {
      if (running === reportedRunning) return;
      reportedRunning = running;
      void syncRunning(running);
    },
    { immediate: true },
  );

  // 弹窗打开期间任务可能自己完成：列表要跟着收缩，全部结束后直接关闭，
  // 不要继续拿一串已完成的任务拦住用户。
  watch(
    () => blockingTasks(),
    (tasks) => {
      if (!exitConfirmVisible.value) return;
      if (tasks.length === 0) {
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
    confirmExit,
    cancelExit,
  };
}
