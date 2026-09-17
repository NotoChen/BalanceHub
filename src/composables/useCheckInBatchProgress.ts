import { computed, onScopeDispose, ref, shallowRef } from "vue";
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

  const tasks = computed(() => {
    const latest = new Map(options.snapshot().items.map((task) => [task.runId, task]));
    return (selected.value?.tasks ?? []).map((task) => {
      const update = latest.get(task.runId);
      return update && update.revision >= task.revision ? update : task;
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

  async function open() {
    if (disposed) return;
    visible.value = true;
    if (progress.value.running) return;

    const snapshot = options.snapshot();
    const active = snapshot.items
      .filter((task) => task.source === "batch" && task.batchId !== null && !task.finished)
      .sort((left, right) => right.startedAt - left.startedAt)[0];
    if (active && active.batchId !== null) {
      const batchTasks = snapshot.items.filter((task) => task.batchId === active.batchId);
      // After a frontend reload only task records survive; the skipped total is unknown.
      selected.value = { batchId: active.batchId, tasks: batchTasks, skipped: null };
      startedAt.value = Math.min(...batchTasks.map((task) => task.startedAt));
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
      else error.value = options.snapshot().error || "签到任务未能提交，请稍后重试";
    } catch (cause) {
      if (!disposed && generation === requestId) {
        error.value = cause instanceof Error ? cause.message : String(cause);
      }
    } finally {
      if (!disposed && generation === requestId) {
        submitting.value = false;
        submissionEndedAt.value = Date.now();
      }
    }
  }

  onScopeDispose(() => {
    disposed = true;
    generation++;
    submitting.value = false;
    visible.value = false;
  });

  return { visible, progress, open };
}
