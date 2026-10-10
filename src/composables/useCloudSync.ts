import {
  computed,
  onMounted,
  onUnmounted,
  provide,
  shallowRef,
  type InjectionKey,
} from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { cloudSyncApi } from "../api/cloud-sync";
import {
  createCloudSyncController,
  type CloudSyncView,
} from "../utils/cloud-sync-controller";
import type {
  BackgroundTask,
  BackgroundTaskStatus,
} from "./useBackgroundTaskCenter";

export type CloudSyncController = ReturnType<typeof useCloudSync>;
export const CLOUD_SYNC_CONTEXT: InjectionKey<CloudSyncController> =
  Symbol("cloud-sync");

export function useCloudSync(onApplied: () => Promise<unknown>) {
  const view = shallowRef<CloudSyncView>({
    snapshot: null,
    pending: "",
    error: "",
    reviewVisible: false,
    comparison: null,
    compareKey: "",
    comparing: false,
    compareError: "",
  });
  const model = createCloudSyncController(cloudSyncApi, (next) => {
    view.value = next;
  });
  let disposed = false;
  let appliedUnlisten: UnlistenFn | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let appliedRevision = 0;
  const controller = {
    state: computed(() => view.value.snapshot),
    pending: computed(() => view.value.pending),
    error: computed(() => view.value.error),
    reviewVisible: computed({
      get: () => view.value.reviewVisible,
      set: model.setReviewVisible,
    }),
    comparison: computed(() => view.value.comparison),
    compareKey: computed(() => view.value.compareKey),
    comparing: computed(() => view.value.comparing),
    compareError: computed(() => view.value.compareError),
    refresh: model.refresh,
    save: model.save,
    sync: model.sync,
    test: model.test,
    cancel: model.cancel,
    restore: model.restore,
    compare: model.compare,
    confirm: model.confirm,
    openReview: model.openReview,
  };
  const tasks = computed<BackgroundTask[]>(() => {
    const status = view.value.snapshot?.status;
    if (!status?.taskId || status.phase === "idle") return [];
    if (
      status.automatic &&
      status.phase === "completed" &&
      status.uploaded === 0 &&
      status.downloaded === 0
    )
      return [];
    const taskStatus: BackgroundTaskStatus = status.running
      ? "running"
      : status.review
        ? "waiting"
        : status.phase === "completed"
          ? "success"
          : status.phase === "cancelled"
            ? "cancelled"
            : "failed";
    const actions: NonNullable<BackgroundTask["actions"]> = [];
    if (status.review)
      actions.push({ label: "查看差异", run: model.openReview });
    if (status.canCancel)
      actions.push({
        label: "取消",
        disabled: Boolean(view.value.pending),
        run: () => {
          void model.cancel();
        },
      });
    if (status.phase === "failed" || status.phase === "cancelled")
      actions.push({
        label: "重新同步",
        disabled: Boolean(view.value.pending),
        run: () => {
          void model.sync();
        },
      });
    return [
      {
        id: status.taskId,
        kind: "cloudSync",
        title: "WebDAV 同步",
        detail: status.message,
        status: taskStatus,
        progress: status.progress,
        startedAt: status.startedAt ?? 0,
        finishedAt: status.finishedAt ?? undefined,
        source: status.automatic ? "automatic" : "manual",
        error: status.phase === "failed" ? status.message : undefined,
        actions,
      },
    ];
  });
  onMounted(async () => {
    void model.start();
    // Recover missed progress events while a background task is active.
    refreshTimer = setInterval(() => {
      if (view.value.snapshot?.status.running && !view.value.pending)
        void model.refresh();
    }, 10_000);
    try {
      const unlisten = await listen<number>("cloud-sync-applied", (event) => {
        if (!disposed && event.payload > appliedRevision) {
          appliedRevision = event.payload;
          void onApplied().catch(() => {});
        }
      });
      if (disposed) unlisten();
      else appliedUnlisten = unlisten;
    } catch {
      /* A failed IPC is presented by the controller. */
    }
  });
  onUnmounted(() => {
    disposed = true;
    model.stop();
    appliedUnlisten?.();
    if (refreshTimer) clearInterval(refreshTimer);
  });
  const exposed = { ...controller, tasks };
  provide(CLOUD_SYNC_CONTEXT, exposed);
  return exposed;
}
