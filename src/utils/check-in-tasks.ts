import type { CheckInBatch, CheckInTask } from "../api/checkin";

export function checkInResumeLabel(task: Pick<CheckInTask, "phase">): string {
  if (task.phase === "waitingBrowser") return "准备浏览器";
  if (task.phase === "waitingLogin") return "继续登录";
  return "继续验证";
}

export interface CheckInSnapshot {
  items: CheckInTask[];
  pending: string[];
  error: string;
  /// 至少成功拿到过一次后端任务全集。首次列表包含前端重载前已结束的
  /// 历史任务，调用方据此区分「历史结果」和「刚刚完成」。
  listed?: boolean;
}

interface CheckInApi {
  list: () => Promise<CheckInTask[]>;
  listen: (receive: (task: CheckInTask) => void) => Promise<() => void>;
  submit: (providerId: string) => Promise<CheckInTask>;
  submitAll: () => Promise<CheckInBatch>;
  resume: (runId: string) => Promise<CheckInTask>;
  cancel: (runId: string) => Promise<void>;
  showWindow: (runId: string) => Promise<void>;
}

export function createCheckInTracker(api: CheckInApi, changed: (snapshot: CheckInSnapshot) => void) {
  const tasks = new Map<string, CheckInTask>();
  const pending = new Map<string, number>();
  let generation = 0;
  let sequence = 0;
  let active = false;
  let unlisten: (() => void) | undefined;
  let error = "";
  /// 本地收到的任务更新批次。列表和事件是两条通道：列表在途期间收到的
  /// 事件比这份快照新，此时不能用列表做「后端全集」清理。
  let received = 0;
  let listed = false;

  const publish = () => {
    if (active) changed({ items: [...tasks.values()], pending: [...pending.keys()], error, listed });
  };
  const apply = (updates: CheckInTask[]) => {
    for (const update of updates) {
      if ((tasks.get(update.runId)?.revision ?? -1) < update.revision) tasks.set(update.runId, update);
    }
  };
  const merge = (updates: CheckInTask[]) => {
    if (updates.length > 0) received++;
    apply(updates);
    publish();
  };
  /// 列表接口是后端当前仍在跟踪的任务全集：Rust 重启或清理掉的未完成任务
  /// 不会再有终态事件，不在这里移除就会永远停在“等待验证”并拦住退出。
  /// 已结束的任务保留在本地下，等 watcher 有空处理完结果和刷新。
  const reconcile = (updates: CheckInTask[]) => {
    const live = new Set(updates.map((update) => update.runId));
    apply(updates);
    for (const [runId, task] of [...tasks.entries()]) {
      if (!live.has(runId) && !task.finished) tasks.delete(runId);
    }
    publish();
  };

  async function refresh() {
    const epoch = generation;
    const receivedBefore = received;
    try {
      const result = await api.list();
      if (active && epoch === generation) {
        error = "";
        listed = true;
        // 列表请求在途时收到的任务事件（例如刚提交的签到）比这份快照新；
        // 此时只合并结果，不让旧快照按「后端已不再跟踪」删掉它们。
        if (received === receivedBefore) reconcile(result);
        else merge(result);
      }
    } catch (cause) {
      if (active && epoch === generation) { error = String(cause); publish(); }
    }
  }

  async function start() {
    active = true;
    const epoch = ++generation;
    try {
      const dispose = await api.listen((task) => {
        if (active && epoch === generation) merge([task]);
      });
      if (!active || epoch !== generation) { dispose(); return; }
      unlisten = dispose;
      await refresh();
    } catch (cause) {
      if (active && epoch === generation) { error = String(cause); publish(); }
    }
  }

  function stop() {
    active = false;
    generation++;
    unlisten?.();
    unlisten = undefined;
    pending.clear();
  }

  async function action<T>(key: string, request: () => Promise<T>, receive: (result: T) => void): Promise<T | undefined> {
    if (pending.has(key)) return;
    const epoch = generation;
    const requestId = ++sequence;
    pending.set(key, requestId);
    error = "";
    publish();
    try {
      const result = await request();
      if (active && generation === epoch) { receive(result); return result; }
    } catch (cause) {
      if (active && generation === epoch) { error = String(cause); publish(); }
    } finally {
      if (pending.get(key) === requestId) pending.delete(key);
      publish();
    }
  }

  return {
    start, stop, refresh,
    submit: (providerId: string) => action(`submit:${providerId}`, () => api.submit(providerId), (task) => merge([task])),
    submitAll: () => action("batch", api.submitAll, (batch) => merge(batch.tasks)),
    resume: (runId: string) => action(`resume:${runId}`, () => api.resume(runId), (task) => merge([task])),
    cancel: (runId: string) => action(`cancel:${runId}`, () => api.cancel(runId), () => { void refresh(); }),
    showWindow: (runId: string) => action(`show:${runId}`, () => api.showWindow(runId), () => {}),
  };
}
