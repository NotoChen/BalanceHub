export type SettingsSaveState = "saved" | "pending" | "saving" | "error";

interface SettingsSaveQueueOptions<T> {
  read: () => T;
  write: (value: T, expected: T) => Promise<T>;
  accept: (value: T) => void;
  state: (state: SettingsSaveState, error: string) => void;
  failed: (message: string) => void;
  delayMs?: number;
  timeoutMs?: number;
}

/** Serializes settings writes and keeps unsaved edits alive across window close/reopen. */
export function createSettingsSaveQueue<T>(options: SettingsSaveQueueOptions<T>) {
  let persisted = settingsSnapshot(options.read());
  let timer: ReturnType<typeof setTimeout> | undefined;
  let active: { waiting: Promise<boolean>; timedOut: boolean } | null = null;
  let replacement: { waiting: Promise<boolean> } | null = null;
  let queued = false;
  let disposed = false;

  const dirty = () => settingsSnapshot(options.read()) !== persisted;
  const hasPendingChanges = () => active !== null || replacement !== null || dirty();

  function clearTimer() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  function schedule() {
    if (disposed) return;
    clearTimer();
    if (active || replacement) {
      queued = true;
      return;
    }
    if (!dirty()) {
      options.state("saved", "");
      return;
    }
    options.state("pending", "");
    timer = setTimeout(() => { timer = undefined; void flush(); }, options.delayMs ?? 300);
  }

  function flush(): Promise<boolean> {
    clearTimer();
    if (disposed) return Promise.resolve(false);
    if (replacement) return replacement.waiting;
    if (active) {
      queued = true;
      return active.waiting;
    }
    if (!dirty()) {
      options.state("saved", "");
      return Promise.resolve(true);
    }

    const value = options.read();
    const expected = JSON.parse(persisted) as T;
    const snapshot = settingsSnapshot(value);
    let finish!: (saved: boolean) => void;
    const current = {
      waiting: new Promise<boolean>((resolve) => { finish = resolve; }),
      timedOut: false,
    };
    active = current;
    queued = false;
    options.state("saving", "");
    const timeout = setTimeout(() => {
      current.timedOut = true;
      if (!disposed) {
        const message = "保存响应超时，修改已保留；应用恢复响应后会继续保存";
        options.state("error", message);
        options.failed(message);
      }
      finish(false);
    }, options.timeoutMs ?? 15_000);

    // A timed-out IPC may still write later. Keep its place in the queue instead
    // of issuing a competing write that could restore older settings on disk.
    void Promise.resolve().then(() => options.write(value, expected)).then((saved) => {
      clearTimeout(timeout);
      active = null;
      if (disposed) { finish(false); return; }
      // The acknowledgement may contain fields received from another device.
      // Rebase edits made while this request was pending onto that result, so
      // the next queued save does not restore untouched, older field values.
      const draft = mergeChangedSettings(value, options.read(), saved);
      persisted = settingsSnapshot(saved);
      options.accept(draft);
      if (dirty()) {
        void flush().then(finish);
      } else {
        options.state("saved", "");
        finish(true);
      }
    }, (error: unknown) => {
      clearTimeout(timeout);
      active = null;
      if (disposed) { finish(false); return; }
      const message = error instanceof Error ? error.message : String(error);
      options.state("error", message);
      if (!current.timedOut) options.failed(message);
      // A later edit deserves its own attempt; a failed unchanged draft does
      // not trigger an endless retry loop.
      if (queued && settingsSnapshot(options.read()) !== snapshot) {
        void flush().then(finish);
      } else {
        finish(false);
      }
    });
    return current.waiting;
  }

  /** A restore shares the write queue; a UI timeout cannot let old drafts overwrite it. */
  function replace<R>(operation: () => Promise<{ settings: T; result: R }>, timeoutMs = 30_000): Promise<R> {
    if (disposed || hasPendingChanges()) {
      return Promise.reject(new Error("应用设置尚未保存，请先重试保存"));
    }
    clearTimer();
    const before = options.read();
    let finish!: (saved: boolean) => void;
    replacement = { waiting: new Promise<boolean>((resolve) => { finish = resolve; }) };
    options.state("saving", "");
    const timeout = setTimeout(() => {
      if (!disposed) options.state("error", "恢复响应超时；新的设置修改已暂存，收到恢复结果后会继续保存");
      finish(false);
    }, timeoutMs);

    return Promise.resolve().then(operation).then(({ settings, result }) => {
      clearTimeout(timeout);
      replacement = null;
      if (disposed) { finish(false); return result; }
      const draft = mergeChangedSettings(before, options.read(), settings);
      persisted = settingsSnapshot(settings);
      options.accept(draft);
      if (dirty()) void flush().then(finish);
      else { options.state("saved", ""); finish(true); }
      return result;
    }, (error: unknown) => {
      clearTimeout(timeout);
      replacement = null;
      finish(false);
      if (!disposed) {
        if (dirty()) void flush();
        else options.state("saved", "");
      }
      throw error;
    });
  }

  function acceptExternal(value: T) {
    if (disposed || hasPendingChanges()) return false;
    clearTimer();
    persisted = settingsSnapshot(value);
    options.state("saved", "");
    return true;
  }

  function dispose() {
    disposed = true;
    clearTimer();
  }

  return { schedule, flush, replace, acceptExternal, hasPendingChanges, dispose };
}

function mergeChangedSettings<T>(before: T, draft: T, restored: T): T {
  if (settingsSnapshot(before) === settingsSnapshot(draft)) return restored;
  if (!isRecord(before) || !isRecord(draft) || !isRecord(restored)) return draft;
  const next: Record<string, unknown> = { ...restored };
  for (const key of new Set([...Object.keys(before), ...Object.keys(draft)])) {
    if (settingsSnapshot(before[key]) === settingsSnapshot(draft[key])) continue;
    if (!(key in draft)) delete next[key];
    else next[key] = mergeChangedSettings(before[key], draft[key], restored[key]);
  }
  return next as T;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function settingsSnapshot(value: unknown) {
  return JSON.stringify(value, (_key, item: unknown) => {
    if (!item || typeof item !== "object" || Array.isArray(item)) return item;
    return Object.fromEntries(Object.entries(item).sort(([left], [right]) => left.localeCompare(right)));
  });
}
