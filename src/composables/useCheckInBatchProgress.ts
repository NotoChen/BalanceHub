import { computed, onScopeDispose, ref, shallowRef, watch } from "vue";
import type { CheckInBatch, CheckInTask } from "../api/checkin";
import type { CheckInSnapshot } from "../utils/check-in-tasks";

export interface CheckInBatchProgress {
  tasks: CheckInTask[];
  submitting: boolean;
  running: boolean;
  completed: boolean;
  skipped: number | null;
  error: string;
  startedAt: number | null;
  finishedAt: number | null;
}

export function useCheckInBatchProgress(options: {
  snapshot: () => CheckInSnapshot;
  submit: () => Promise<CheckInBatch | undefined>;
}) {
  const visible = ref(false);
  const submitting = ref(false);
  const selected = shallowRef<(Omit<CheckInBatch, "skipped"> & { skipped: number | null }) | null>(null);
  const error = ref("");
  const startedAt = ref<number | null>(null);
  const submissionEndedAt = ref<number | null>(null);
  let generation = 0;
  let disposed = false;

  /// 本批次里已经在任务快照中出现过的任务。只有「出现过、随后又消失」才能
  /// 判定后端已不再跟踪；从未出现过的任务可能只是快照还没追上，不能误判。
  const seenRunIds = new Set<string>();
  let seenBatchId = "";
  const tasks = computed(() => {
    const latest = new Map(options.snapshot().items.map((task) => [task.runId, task]));
    const batchId = selected.value?.batchId ?? "";
    if (batchId !== seenBatchId) {
      seenBatchId = batchId;
      seenRunIds.clear();
    }
    return (selected.value?.tasks ?? []).map((task) => {
      const update = latest.get(task.runId);
      if (update) seenRunIds.add(task.runId);
      const current = update && update.revision >= task.revision ? update : task;
      if (update || current.finished || !seenRunIds.has(task.runId)) return current;
      // 后端任务全集里已经没有这条任务（应用后端重启或任务被清理），不会再
      // 有任何终态事件；继续按未完成渲染会让整批进度永远停在运行中。
      const lost: CheckInTask = {
        ...current,
        phase: "unconfirmed",
        message: "任务已从后台任务列表消失（应用可能已重启），请查看站点记录后重新签到",
        finished: true,
        canResume: false,
        canCancel: false,
        canShowWindow: false,
      };
      return lost;
    });
  });
  const progress = computed<CheckInBatchProgress>(() => {
    const completed = selected.value !== null && !submitting.value && tasks.value.every((task) => task.finished);
    const finishedTimes = tasks.value.flatMap((task) => task.finishedAt === null ? [] : [task.finishedAt]);
    return {
      tasks: tasks.value,
      submitting: submitting.value,
      running: submitting.value || tasks.value.some((task) => !task.finished),
      completed,
      skipped: selected.value?.skipped ?? null,
      error: error.value,
      startedAt: startedAt.value,
      finishedAt: completed
        ? (finishedTimes.length ? Math.max(...finishedTimes) : submissionEndedAt.value)
        : error.value ? submissionEndedAt.value : null,
    };
  });

  /// 把后端仍在跟踪的批次接管到当前弹窗。前端重载后只剩任务记录，
  /// skipped 总数未知；提交响应丢失时也只能靠任务记录恢复进度。
  function adoptActiveBatch(snapshot: CheckInSnapshot) {
    const active = snapshot.items
      .filter((task) => task.source === "batch" && task.batchId !== null && !task.finished)
      .sort((left, right) => right.startedAt - left.startedAt)[0];
    if (!active || active.batchId === null) return false;
    const batchTasks = snapshot.items.filter((task) => task.batchId === active.batchId);
    selected.value = { batchId: active.batchId, tasks: batchTasks, skipped: null };
    startedAt.value = Math.min(...batchTasks.map((task) => task.startedAt));
    return true;
  }

  async function open() {
    if (disposed) return;
    visible.value = true;
    if (progress.value.running) return;

    const snapshot = options.snapshot();
    if (adoptActiveBatch(snapshot)) {
      error.value = "";
      return;
    }
    if (snapshot.pending.includes("batch")) return;

    const requestId = ++generation;
    selected.value = null;
    error.value = "";
    startedAt.value = Date.now();
    submissionEndedAt.value = null;
    submitting.value = true;
    try {
      const batch = await options.submit();
      if (disposed || generation !== requestId) return;
      if (batch) selected.value = batch;
      else if (adoptActiveBatch(options.snapshot())) {
        // 提交响应超时或丢失，但后端已经受理：直接接管正在运行的批次，
        // 不能停在“提交失败”而任务其实在跑。
        error.value = "";
      } else {
        error.value = options.snapshot().error || "签到任务未能提交，请稍后重试";
      }
    } catch (cause) {
      if (!disposed && generation === requestId) {
        if (adoptActiveBatch(options.snapshot())) error.value = "";
        else error.value = cause instanceof Error ? cause.message : String(cause);
      }
    } finally {
      if (!disposed && generation === requestId) {
        submitting.value = false;
        submissionEndedAt.value = Date.now();
      }
    }
  }

  // 提交超时后批次可能稍晚才出现在后端任务列表里；一旦看到运行中的
  // 批次就接管，撤掉已经过时的提交错误。
  watch(
    () => options.snapshot(),
    (snapshot) => {
      if (disposed || !visible.value || selected.value !== null || !error.value) return;
      if (adoptActiveBatch(snapshot)) error.value = "";
    },
  );

  onScopeDispose(() => {
    disposed = true;
    generation++;
    submitting.value = false;
    visible.value = false;
  });

  return { visible, progress, open };
}
