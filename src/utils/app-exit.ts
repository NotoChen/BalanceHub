import type { BackgroundTask } from "../composables/useBackgroundTaskCenter";

/// 只读探测（公告、CLI 环境、更新查询）可以在退出时直接放弃，不需要拿一个
/// 确认弹窗拦住用户；真正会丢进度或中断外部会话的任务才进入退出确认。
const nonBlockingTaskIds = new Set([
  "site-announcements",
  "cli-runtime-probe",
  "update-check",
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
      !nonBlockingTaskIds.has(task.id) &&
      !nonBlockingTaskKinds.has(task.kind),
  );
}

/// 弹窗标题只报数量；每条任务的标题和细节由列表承担，避免长句挤在一行。
export function exitTaskSummary(tasks: readonly BackgroundTask[]): string {
  if (tasks.length === 0) return "当前没有运行中的任务";
  return `${tasks.length} 个任务仍在运行`;
}
