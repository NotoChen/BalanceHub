import { onMounted, onUnmounted, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "../api/transport.ts";
import { useHostPlatform } from "./useHostPlatform";
import type { AppSettings } from "../stores/providers";

interface UseAppLifecycleOptions {
  loadError: Ref<string | null>;
  settings: Ref<AppSettings>;
  settingsForm: AppSettings;
  settingsDrawerVisible: Ref<boolean>;
  initialize: () => Promise<unknown>;
  syncFromSettings: (settings?: AppSettings) => void;
  setupThemeListener: () => void;
  cleanupThemeListener: () => void;
  syncLaunchAtLogin: () => Promise<unknown>;
  autoProbeCliTools: () => Promise<unknown>;
  /// 后端调度任务变更状态后会发出 `providers-changed` 事件，前端据此重新拉取内存状态。
  reloadProviders: () => Promise<unknown> | unknown;
  /// 运行期存储进入保护态（例如云同步落盘失败）：保存已暂停，只能重启恢复。
  onStorageBlocked: (error: string) => void;
  applyTheme: (themeMode: AppSettings["themeMode"]) => void;
  flushSettingsSave: () => void | Promise<unknown>;
  resetProviderPointerDrag: (suppressClick: boolean, preserveDragOrder?: boolean) => void;
}

export function useAppLifecycle(options: UseAppLifecycleOptions) {
  const host = useHostPlatform();
  let providersChangedUnlisten: UnlistenFn | null = null;
  let eventsReconnectedUnlisten: UnlistenFn | null = null;
  let storageBlockedUnlisten: UnlistenFn | null = null;
  let disposed = false;
  let postLoadStarted = false;

  /// 读取配置失败时这里会先跳过；用户修好 data.json 点「重新读取」后，
  /// loadError 清空，启动期只做一次的动作需要补上，不能等下次重启。
  async function startPostLoadTasks() {
    if (disposed || postLoadStarted || options.loadError.value) return;
    postLoadStarted = true;
    await options.syncLaunchAtLogin();
    if (disposed) return;
    await options.autoProbeCliTools();
  }

  onMounted(async () => {
    disposed = false;
    const platform = await host.ensure();
    if (disposed) return;
    if (platform) {
      document.documentElement.classList.remove(
        "platform-macos",
        "platform-windows",
        "platform-linux",
      );
      document.documentElement.classList.add(
        `platform-${platform === "macos" ? "macos" : platform}`,
      );
    }

    await options.initialize();
    if (disposed) return;
    options.syncFromSettings();
    options.setupThemeListener();
    try {
      // 监听后端调度任务的状态变更，自动刷新视图（关窗到托盘时也能保持同步）。
      const unlisten = await listen("providers-changed", () => {
        if (!disposed) {
          void options.reloadProviders();
        }
      });
      if (disposed) {
        unlisten();
        return;
      }
      providersChangedUnlisten = unlisten;
      // 服务端事件流断线重连后，断开期间的状态变更已经错过；重新拉取一次。
      const unlistenReconnect = await listen(
        "balancehub-events-reconnected",
        () => {
          if (!disposed) {
            void options.reloadProviders();
          }
        },
      );
      if (disposed) {
        unlistenReconnect();
        return;
      }
      eventsReconnectedUnlisten = unlistenReconnect;
      // 只在运行期出现：启动读取失败由 initialize() 的 loadError 负责。
      const unlistenBlocked = await listen<string>("storage-blocked", (event) => {
        if (!disposed) options.onStorageBlocked(event.payload);
      });
      if (disposed) unlistenBlocked();
      else storageBlockedUnlisten = unlistenBlocked;
    } catch {
      // Browser preview has no Tauri backend; scheduling events are unavailable.
    }
    await startPostLoadTasks();
  });

  watch(options.loadError, (error) => {
    if (!error) void startPostLoadTasks();
  });

  onUnmounted(() => {
    disposed = true;
    options.cleanupThemeListener();
    options.resetProviderPointerDrag(false);
    providersChangedUnlisten?.();
    providersChangedUnlisten = null;
    eventsReconnectedUnlisten?.();
    eventsReconnectedUnlisten = null;
    storageBlockedUnlisten?.();
    storageBlockedUnlisten = null;
  });

  watch(options.settings, (value) => {
    // The settings controller protects pending edits. A clean open window must
    // still receive cloud changes and delayed provider reloads.
    options.syncFromSettings(value);
  });

  watch(
    () => options.settingsForm.themeMode,
    (value) => options.applyTheme(value),
  );

  watch(options.settingsDrawerVisible, (visible) => {
    if (!visible) {
      void options.flushSettingsSave();
    }
  });

}
