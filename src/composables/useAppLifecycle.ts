import { onMounted, onUnmounted, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hostPlatform } from "../api/app";
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
  applyTheme: (themeMode: AppSettings["themeMode"]) => void;
  flushSettingsSave: () => void | Promise<unknown>;
  resetProviderPointerDrag: (suppressClick: boolean, preserveDragOrder?: boolean) => void;
}

export function useAppLifecycle(options: UseAppLifecycleOptions) {
  let providersChangedUnlisten: UnlistenFn | null = null;
  let disposed = false;

  async function resolveHostPlatform() {
    try {
      return await hostPlatform();
    } catch {
      // Browser preview has no Tauri backend; keep the default macOS-aligned spacing.
      return null;
    }
  }

  onMounted(async () => {
    disposed = false;
    const platform = await resolveHostPlatform();
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
    } catch {
      // Browser preview has no Tauri backend; scheduling events are unavailable.
    }
    if (disposed || options.loadError.value) {
      return;
    }
    await options.syncLaunchAtLogin();
    if (disposed) return;
    await options.autoProbeCliTools();
  });

  onUnmounted(() => {
    disposed = true;
    options.cleanupThemeListener();
    options.resetProviderPointerDrag(false);
    providersChangedUnlisten?.();
    providersChangedUnlisten = null;
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
