import type { BackgroundTask } from "../composables/useBackgroundTaskCenter";

/// 只读探测（公告、CLI 环境、更新查询）可以在退出时直接放弃，不需要拿一个
/// 确认弹窗拦住用户；真正会丢进度或中断外部会话的任务才进入退出确认。
/// 自动维护（自动刷新、自动测活等）同理通过 ambient 标记排除。
/// 两条 *-connection 是事件流断线的状态提示，不是正在执行的任务：退出时
/// 它们会随进程重建，不能因为没有「完成」按钮就永远拦在退出确认里。
const nonBlockingTaskIds = new Set([
  "site-announcements",
  "cli-runtime-probe",
  "update-check",
  "oauth:connection",
  "agent-lifecycle-connection",
  // 存储保护态要重启才能解除，但用户始终可以选择退出后自己重新打开；
  // 退出确认不能把它当成「仍在运行的任务」。
  "storage-blocked",
]);
const nonBlockingTaskKinds = new Set<BackgroundTask["kind"]>([
  "announcement",
  "cliProbe",
]);

export function exitBlockingTasks(
  tasks: readonly BackgroundTask[],
): BackgroundTask[] {
  return tasks.filter(
    (task) =>
      !task.ambient &&
      !nonBlockingTaskIds.has(task.id) &&
      !nonBlockingTaskKinds.has(task.kind),
  );
}

/// 弹窗标题只报数量；每条任务的标题和细节由列表承担，避免长句挤在一行。
export function exitTaskSummary(tasks: readonly BackgroundTask[]): string {
  if (tasks.length === 0) return "当前没有运行中的任务";
  return `${tasks.length} 个任务仍在运行`;
}
