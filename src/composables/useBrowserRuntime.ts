import { computed, onMounted, onUnmounted, provide, shallowRef, type InjectionKey } from "vue";
import { supportsCommand } from "./useHostCapabilities";
import { cancelBrowserRuntimeInstall, getBrowserRuntimeStatus, installBrowserRuntime, listenBrowserRuntime, selectBrowserRuntimeBrowser, uninstallBrowserRuntime } from "../api/browser-runtime";
import { createBrowserRuntimeController, type BrowserRuntimeSnapshot } from "../utils/browser-runtime-controller";

export type BrowserRuntimeController = ReturnType<typeof useBrowserRuntime>;
export const BROWSER_RUNTIME_CONTEXT: InjectionKey<BrowserRuntimeController> = Symbol("browser-runtime");

export function useBrowserRuntime() {
  const snapshot = shallowRef<BrowserRuntimeSnapshot>({ status: null, visible: false, pending: "", error: "" });
  const model = createBrowserRuntimeController({ status: getBrowserRuntimeStatus, listen: listenBrowserRuntime,
    install: installBrowserRuntime, selectBrowser: selectBrowserRuntimeBrowser, cancel: cancelBrowserRuntimeInstall, uninstall: uninstallBrowserRuntime,
  }, (next) => { snapshot.value = next; });
  // 宿主没有浏览器组件命令时不探测、不监听，界面也不展示相关面板。
  const available = supportsCommand("get_browser_runtime_status");
  onMounted(() => { if (available) void model.start().catch(() => {}); });
  onUnmounted(model.stop);
  const controller = {
    available: computed(() => supportsCommand("get_browser_runtime_status")),
    state: computed(() => snapshot.value.status),
    visible: computed({ get: () => snapshot.value.visible, set: model.setVisible }),
    pending: computed(() => snapshot.value.pending),
    error: computed(() => snapshot.value.error),
    refresh: model.refresh, install: model.install, selectBrowser: model.selectBrowser, cancel: model.cancel, uninstall: model.uninstall, open: model.open,
  };
  provide(BROWSER_RUNTIME_CONTEXT, controller);
  return controller;
}
