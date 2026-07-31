import { onMounted, onUnmounted, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hostPlatform } from "../api/app";
import type { AppSettings, Provider } from "../stores/providers";
import type { UsagePeriod } from "../utils/usage-trend";

interface UseAppLifecycleOptions {
  loadError: Ref<string | null>;
  settings: Ref<AppSettings>;
  settingsForm: AppSettings;
  settingsDrawerVisible: Ref<boolean>;
  usageVisible: Ref<boolean>;
  usageProvider: Ref<Provider | null>;
  usagePeriod: Ref<UsagePeriod>;
  checkInRecordsVisible: Ref<boolean>;
  checkInRecordsProviderId: Ref<string | null>;
  checkInRecordsMonth: Ref<string>;
  initialize: () => Promise<unknown>;
  syncFromSettings: (settings?: AppSettings) => void;
  setupThemeListener: () => void;
  cleanupThemeListener: () => void;
  syncLaunchAtLogin: () => Promise<unknown>;
  autoProbeCliEnvironment: () => Promise<unknown>;
  /// 后端调度任务变更状态后会发出 `providers-changed` 事件，前端据此重新拉取内存状态。
  reloadProviders: () => Promise<unknown> | unknown;
  applyTheme: (themeMode: AppSettings["themeMode"]) => void;
  resetSettingsDraft: () => void | Promise<unknown>;
  resetProviderPointerDrag: (suppressClick: boolean, preserveDragOrder?: boolean) => void;
  refreshUsageSummary: () => Promise<unknown> | unknown;
  loadCheckInRecords: () => Promise<unknown> | unknown;
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
    await options.autoProbeCliEnvironment();
  });

  onUnmounted(() => {
    disposed = true;
    options.cleanupThemeListener();
    options.resetProviderPointerDrag(false);
    providersChangedUnlisten?.();
    providersChangedUnlisten = null;
  });

  watch(options.settings, (value) => {
    // 设置草稿会实时写入；设置窗口打开时仍避免后台状态回灌覆盖正在编辑的控件。
    if (options.settingsDrawerVisible.value) {
      return;
    }
    options.syncFromSettings(value);
  });

  watch(
    () => options.settingsForm.themeMode,
    (value) => options.applyTheme(value),
  );

  watch(options.settingsDrawerVisible, (visible) => {
    if (!visible) {
      void options.resetSettingsDraft();
    }
  });

  watch(options.usagePeriod, () => {
    if (options.usageVisible.value && options.usageProvider.value) {
      void options.refreshUsageSummary();
    }
  });

  watch([options.checkInRecordsMonth, options.checkInRecordsVisible], () => {
    if (options.checkInRecordsVisible.value && options.checkInRecordsProviderId.value) {
      void options.loadCheckInRecords();
    }
  });
}
