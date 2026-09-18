import type { CheckInBatch, CheckInTask } from "../api/checkin";

export interface CheckInSnapshot {
  items: CheckInTask[];
  pending: string[];
  error: string;
}

interface CheckInApi {
  list: () => Promise<CheckInTask[]>;
  listen: (receive: (task: CheckInTask) => void) => Promise<() => void>;
  submit: (providerId: string) => Promise<CheckInTask>;
  submitAll: () => Promise<CheckInBatch>;
  resume: (runId: string) => Promise<CheckInTask>;
  cancel: (runId: string) => Promise<void>;
}

export function createCheckInTracker(api: CheckInApi, changed: (snapshot: CheckInSnapshot) => void) {
  const tasks = new Map<string, CheckInTask>();
  const pending = new Map<string, number>();
  let generation = 0;
  let sequence = 0;
  let active = false;
  let unlisten: (() => void) | undefined;
  let error = "";

  const publish = () => {
    if (active) changed({ items: [...tasks.values()], pending: [...pending.keys()], error });
  };
  const merge = (updates: CheckInTask[]) => {
    for (const update of updates) {
      if ((tasks.get(update.runId)?.revision ?? -1) < update.revision) tasks.set(update.runId, update);
    }
    publish();
  };

  async function refresh() {
    const epoch = generation;
    try {
      const result = await api.list();
      if (active && epoch === generation) { error = ""; merge(result); }
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
  };
}
