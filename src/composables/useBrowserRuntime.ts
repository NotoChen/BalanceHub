import { computed, onMounted, onUnmounted, provide, shallowRef, type InjectionKey } from "vue";
import { listen, type UnlistenFn } from "../api/transport.ts";
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
  let disposed = false;
  let reconnectUnlisten: UnlistenFn | null = null;
  onMounted(() => {
    if (available) void model.start().catch(() => {});
    // 组件状态只随后端事件更新。服务端重启后安装进度不会再有事件，
    // 任务中心会一直停在「安装浏览器辅助组件」，面板也停在旧阶段；
    // 事件流重连时补读一次当前状态（控制器仍按版本号丢弃过期结果）。
    void listen("balancehub-events-reconnected", () => {
      if (!disposed && available) void model.refresh();
    }).then((stop) => {
      if (disposed) stop();
      else reconnectUnlisten = stop;
    }).catch(() => {
      // 桌面版没有这条补偿事件，状态由同一进程直接维护。
    });
  });
  onUnmounted(() => {
    disposed = true;
    reconnectUnlisten?.();
    reconnectUnlisten = null;
    model.stop();
  });
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
