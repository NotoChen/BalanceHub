import type {
  CloudSyncSnapshot,
  CloudSyncComparison,
  CloudSyncResolution,
  CloudSyncSettingsInput,
} from "../api/cloud-sync";

interface CloudSyncApi {
  state: () => Promise<CloudSyncSnapshot>;
  save: (input: CloudSyncSettingsInput) => Promise<CloudSyncSnapshot>;
  sync: () => Promise<CloudSyncSnapshot>;
  test: () => Promise<CloudSyncSnapshot>;
  restore: () => Promise<CloudSyncSnapshot>;
  cancel: (taskId: string) => Promise<CloudSyncSnapshot>;
  confirm: (
    reviewId: string,
    resolutions: CloudSyncResolution[],
  ) => Promise<CloudSyncSnapshot>;
  compare: (reviewId: string, key: string) => Promise<CloudSyncComparison>;
  listen: (
    receive: (snapshot: CloudSyncSnapshot) => void,
  ) => Promise<() => void>;
}
export interface CloudSyncView {
  snapshot: CloudSyncSnapshot | null;
  pending: string;
  error: string;
  reviewVisible: boolean;
  comparison: CloudSyncComparison | null;
  compareKey: string;
  comparing: boolean;
  compareError: string;
}

export function createCloudSyncController(
  api: CloudSyncApi,
  changed: (view: CloudSyncView) => void,
) {
  let view: CloudSyncView = {
    snapshot: null,
    pending: "",
    error: "",
    reviewVisible: false,
    comparison: null,
    compareKey: "",
    comparing: false,
    compareError: "",
  };
  let active = true;
  let operation = 0;
  let comparisonRequest = 0;
  let unlisten: (() => void) | undefined;
  const publish = () => {
    if (active) changed({ ...view });
  };
  function invalidateComparison() {
    comparisonRequest++;
    view.comparison = null;
    view.compareKey = "";
    view.comparing = false;
    view.compareError = "";
  }
  function apply(snapshot: CloudSyncSnapshot) {
    if (
      !active ||
      snapshot.status.revision <= (view.snapshot?.status.revision ?? -1)
    )
      return;
    if (view.snapshot?.status.review?.id !== snapshot.status.review?.id) {
      invalidateComparison();
      if (snapshot.status.review && !snapshot.status.automatic)
        view.reviewVisible = true;
    }
    view.snapshot = snapshot;
    if (!snapshot.status.review) view.reviewVisible = false;
    publish();
  }
  async function run(label: string, request: () => Promise<CloudSyncSnapshot>) {
    if (view.pending || !active) return false;
    const id = ++operation;
    view.pending = label;
    view.error = "";
    publish();
    try {
      const snapshot = await request();
      if (active && operation === id) {
        apply(snapshot);
        return true;
      }
      return false;
    } catch (error) {
      if (active && operation === id) {
        view.error = error instanceof Error ? error.message : String(error);
      }
      return false;
    } finally {
      if (id === operation) {
        view.pending = "";
        publish();
      }
    }
  }
  const refresh = () => run("refresh", api.state);
  function setReviewVisible(visible: boolean) {
    view.reviewVisible = visible && Boolean(view.snapshot?.status.review);
    if (!visible) invalidateComparison();
    publish();
  }
  async function compare(key: string) {
    const reviewId = view.snapshot?.status.review?.id;
    if (!active || !view.reviewVisible || !reviewId) return;
    const requestId = ++comparisonRequest;
    view.compareKey = key;
    view.comparing = true;
    view.compareError = "";
    view.comparison = null;
    publish();
    try {
      const result = await api.compare(reviewId, key);
      if (
        active &&
        requestId === comparisonRequest &&
        view.reviewVisible &&
        view.snapshot?.status.review?.id === reviewId
      )
        view.comparison = result;
    } catch (error) {
      if (active && requestId === comparisonRequest)
        view.compareError =
          error instanceof Error ? error.message : String(error);
    } finally {
      if (requestId === comparisonRequest) {
        view.comparing = false;
        publish();
      }
    }
  }
  async function start() {
    try {
      const dispose = await api.listen(apply);
      if (!active) {
        dispose();
        return;
      }
      unlisten = dispose;
    } catch (error) {
      if (active) {
        view.error = String(error);
        publish();
      }
    }
    if (active) await refresh();
  }
  function stop() {
    active = false;
    operation++;
    comparisonRequest++;
    unlisten?.();
    unlisten = undefined;
  }
  return {
    start,
    stop,
    refresh,
    compare,
    setReviewVisible,
    openReview: () => setReviewVisible(true),
    save: (input: CloudSyncSettingsInput) => run("save", () => api.save(input)),
    sync: () => run("sync", api.sync),
    test: () => run("test", api.test),
    restore: () => run("restore", api.restore),
    cancel: () => {
      const id = view.snapshot?.status.taskId;
      return id ? run("cancel", () => api.cancel(id)) : Promise.resolve(false);
    },
    confirm: (resolutions: CloudSyncResolution[]) => {
      const id = view.snapshot?.status.review?.id;
      if (!id || view.pending) return Promise.resolve(false);
      setReviewVisible(false);
      return run("confirm", () => api.confirm(id, resolutions)).then((ok) => {
        if (!ok && active && view.snapshot?.status.review?.id === id)
          setReviewVisible(true);
        return ok;
      });
    },
  };
}
