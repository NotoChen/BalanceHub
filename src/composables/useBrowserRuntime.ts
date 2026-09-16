import { computed, onMounted, onUnmounted, provide, shallowRef, type InjectionKey } from "vue";
import { cancelBrowserRuntimeInstall, getBrowserRuntimeStatus, installBrowserRuntime, listenBrowserRuntime, uninstallBrowserRuntime } from "../api/browser-runtime";
import { createBrowserRuntimeController, type BrowserRuntimeSnapshot } from "../utils/browser-runtime-controller";

export type BrowserRuntimeController = ReturnType<typeof useBrowserRuntime>;
export const BROWSER_RUNTIME_CONTEXT: InjectionKey<BrowserRuntimeController> = Symbol("browser-runtime");

export function useBrowserRuntime() {
  const snapshot = shallowRef<BrowserRuntimeSnapshot>({ status: null, visible: false, pending: "", error: "" });
  const model = createBrowserRuntimeController({ status: getBrowserRuntimeStatus, listen: listenBrowserRuntime,
    install: installBrowserRuntime, cancel: cancelBrowserRuntimeInstall, uninstall: uninstallBrowserRuntime,
  }, (next) => { snapshot.value = next; });
  onMounted(() => { void model.start().catch(() => {}); });
  onUnmounted(model.stop);
  const controller = {
    state: computed(() => snapshot.value.status),
    visible: computed({ get: () => snapshot.value.visible, set: model.setVisible }),
    pending: computed(() => snapshot.value.pending),
    error: computed(() => snapshot.value.error),
    refresh: model.refresh, install: model.install, cancel: model.cancel, uninstall: model.uninstall, open: model.open,
  };
  provide(BROWSER_RUNTIME_CONTEXT, controller);
  return controller;
}
