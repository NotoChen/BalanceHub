import type { BrowserRuntimeStatus } from "../api/browser-runtime";

export interface BrowserRuntimeSnapshot {
  status: BrowserRuntimeStatus | null;
  visible: boolean;
  pending: string;
  error: string;
}
interface BrowserRuntimeApi {
  status: (force: boolean) => Promise<BrowserRuntimeStatus>;
  listen: (receive: (status: BrowserRuntimeStatus) => void) => Promise<() => void>;
  install: (includeBrowser: boolean) => Promise<BrowserRuntimeStatus>;
  cancel: () => Promise<void>;
  uninstall: () => Promise<BrowserRuntimeStatus>;
}

export function createBrowserRuntimeController(api: BrowserRuntimeApi, changed: (snapshot: BrowserRuntimeSnapshot) => void) {
  let snapshot: BrowserRuntimeSnapshot = { status: null, visible: false, pending: "", error: "" };
  let active = true;
  let sequence = 0;
  let unlisten: (() => void) | undefined;
  const publish = () => { if (active) changed({ ...snapshot }); };
  function apply(status: BrowserRuntimeStatus) {
    if (active && status.revision > (snapshot.status?.revision ?? -1)) { snapshot.status = status; publish(); }
  }
  async function run(label: string, request: () => Promise<BrowserRuntimeStatus | void>) {
    if (snapshot.pending) return;
    const requestId = ++sequence;
    snapshot.pending = label;
    snapshot.error = "";
    publish();
    try {
      const result = await request();
      if (active && requestId === sequence && result) apply(result);
    } catch (error) {
      if (active && requestId === sequence) snapshot.error = String(error);
    } finally {
      if (requestId === sequence) { snapshot.pending = ""; publish(); }
    }
  }
  const refresh = (force = false) => run("detect", () => api.status(force));
  function setVisible(visible: boolean) { snapshot.visible = visible; publish(); }
  async function start() {
    const dispose = await api.listen(apply);
    if (!active) { dispose(); return; }
    unlisten = dispose;
    await refresh();
  }
  function stop() { active = false; sequence++; unlisten?.(); unlisten = undefined; }
  return { start, stop, refresh, setVisible,
    open: () => { setVisible(true); void refresh(); },
    install: (includeBrowser: boolean) => run("install", () => api.install(includeBrowser)),
    cancel: () => run("cancel", api.cancel),
    uninstall: () => run("uninstall", api.uninstall),
  };
}
