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
  /// 事件只携带了受影响的站点 id 时，只补这些卡片；没有提供时回落为整份快照。
  reloadProviderViews?: (ids: string[]) => Promise<unknown> | unknown;
  /// 运行期存储进入保护态（例如云同步落盘失败）：保存已暂停，只能重启恢复。
  onStorageBlocked: (error: string) => void;
  /// 外部 config.json 已成功热重载（数据本身由 providers-changed 拉取）。
  onConfigReloaded?: (result: {
    path: string;
    providerCount: number | null;
    settingsChanged: boolean;
  }) => void;
  /// 外部 config.json 无法解析或校验失败；当前配置保持不变。
  onConfigReloadFailed?: (result: { path: string; error: string }) => void;
  applyTheme: (themeMode: AppSettings["themeMode"]) => void;
  flushSettingsSave: () => void | Promise<unknown>;
  resetProviderPointerDrag: (suppressClick: boolean, preserveDragOrder?: boolean) => void;
}

export function useAppLifecycle(options: UseAppLifecycleOptions) {
  const host = useHostPlatform();
  let providersChangedUnlisten: UnlistenFn | null = null;
  let eventsReconnectedUnlisten: UnlistenFn | null = null;
  let storageBlockedUnlisten: UnlistenFn | null = null;
  let configReloadedUnlisten: UnlistenFn | null = null;
  let configReloadFailedUnlisten: UnlistenFn | null = null;
  let disposed = false;
  let postLoadStarted = false;
  let providerReloadDeferred = false;
  /// 隐藏期间只记账：定向事件记住站点 id，整份变更记一个标记，恢复可见
  /// 后一次补齐。签到批量完成时不必隐藏一次就补拉几十遍。
  let deferredFullProviderReload = false;
  const deferredProviderIds = new Set<string>();
  /// 同一帧内到达的定向事件合并成一次补拉：批量签到完成时几十条事件
  /// 不必各发一次 IPC，也不会各触发一轮卡片合并。
  let pendingProviderIds = new Set<string>();
  let providerFlushTimer: ReturnType<typeof setTimeout> | null = null;

  /// 窗口在托盘或后台时没有界面消费这份快照；先记账，恢复可见后再补拉，
  /// 避免隐藏期间为每条后台事件反复解析、替换整份配置。
  function requestProviderReload(providerIds?: string[]) {
    if (disposed) return;
    const targeted = providerIds && providerIds.length > 0 ? providerIds : null;
    if (typeof document !== "undefined" && document.visibilityState === "hidden") {
      providerReloadDeferred = true;
      if (targeted) {
        for (const id of targeted) deferredProviderIds.add(id);
      } else {
        deferredFullProviderReload = true;
      }
      return;
    }
    // 整份快照由 store 自己单飞合并；丢弃待补的定向 id，避免随后再补一遍。
    if (!targeted || !options.reloadProviderViews) {
      pendingProviderIds.clear();
      clearProviderFlushTimer();
      // 完整快照覆盖所有站点，隐藏期间记下的定向 id 不必再补一次。
      deferredProviderIds.clear();
      deferredFullProviderReload = false;
      providerReloadDeferred = false;
      void options.reloadProviders();
      return;
    }
    for (const id of targeted) pendingProviderIds.add(id);
    if (providerFlushTimer !== null) return;
    providerFlushTimer = setTimeout(() => {
      providerFlushTimer = null;
      const ids = [...pendingProviderIds];
      pendingProviderIds.clear();
      if (disposed || ids.length === 0) return;
      // 排队的这一帧里窗口可能刚好被隐藏，仍然转交给恢复可见时的补拉。
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        providerReloadDeferred = true;
        for (const id of ids) deferredProviderIds.add(id);
        return;
      }
      void options.reloadProviderViews?.(ids);
    }, 16);
  }

  function clearProviderFlushTimer() {
    if (providerFlushTimer === null) return;
    clearTimeout(providerFlushTimer);
    providerFlushTimer = null;
  }

  function flushDeferredProviderReload() {
    if (disposed || !providerReloadDeferred) return;
    if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
    providerReloadDeferred = false;
    const ids = [...deferredProviderIds];
    deferredProviderIds.clear();
    const full = deferredFullProviderReload;
    deferredFullProviderReload = false;
    if (!full && ids.length > 0 && options.reloadProviderViews) {
      void options.reloadProviderViews(ids);
      return;
    }
    void options.reloadProviders();
  }

  /// 事件载荷：带 providerIds 表示只改了这些站点；null/缺省表示设置、
  /// 登录账号或整份配置可能已经变化，需要完整快照。
  function providerIdsFromPayload(payload: unknown) {
    if (!payload || typeof payload !== "object") return undefined;
    const ids = (payload as { providerIds?: unknown }).providerIds;
    if (!Array.isArray(ids)) return undefined;
    return ids.filter((id): id is string => typeof id === "string" && id.length > 0);
  }

  /// 读取配置失败时这里会先跳过；用户修好 config.json 点「重新读取」后，
  /// loadError 清空，启动期只做一次的动作需要补上，不能等下次重启。
  async function startPostLoadTasks() {
    if (disposed || postLoadStarted || options.loadError.value) return;
    postLoadStarted = true;
    // 开机启动状态回读和 CLI 探测互不依赖，并行执行；探测可能启动子进程，
    // 不该排在一次系统状态回读后面串行等待。两者都自行吞掉可预期错误。
    await Promise.all([options.syncLaunchAtLogin(), options.autoProbeCliTools()]);
  }

  onMounted(async () => {
    disposed = false;
    // 隐藏期间推迟的补拉在窗口恢复时执行；focus 兜底没有触发
    // visibilitychange 的宿主。
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", flushDeferredProviderReload);
    }
    if (typeof window !== "undefined") {
      window.addEventListener("focus", flushDeferredProviderReload);
    }
    // 读取系统类型和加载本地配置互不依赖，同时发起，少等一次串行 IPC；
    // 平台样式仍先于配置渲染应用，避免首屏闪一下错误的平台样式。
    const platformPromise = host.ensure();
    const initializePromise = options.initialize();
    const platform = await platformPromise;
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

    await initializePromise;
    if (disposed) return;
    options.syncFromSettings();
    options.setupThemeListener();
    try {
      // 五个订阅互不依赖，并行注册，不再逐个等待 IPC 往返；浏览器预览没有
      // Tauri 后端时每个订阅单独失败，不影响已成功的订阅继续工作。
      const results = await Promise.allSettled([
        // 监听后端调度任务的状态变更，自动刷新视图（关窗到托盘时也能保持同步）。
        listen<unknown>("providers-changed", (event) => {
          requestProviderReload(providerIdsFromPayload(event.payload));
        }),
        // 服务端事件流断线重连后，断开期间的状态变更已经错过；重新拉取一次。
        listen("balancehub-events-reconnected", () => requestProviderReload()),
        // 只在运行期出现：启动读取失败由 initialize() 的 loadError 负责。
        listen<string>("storage-blocked", (event) => {
          if (!disposed) options.onStorageBlocked(event.payload);
        }),
        listen<{
          path: string;
          providerCount: number | null;
          settingsChanged: boolean;
        }>("config-reloaded", (event) => {
          if (!disposed) options.onConfigReloaded?.(event.payload);
        }),
        listen<{ path: string; error: string }>(
          "config-reload-failed",
          (event) => {
            if (!disposed) options.onConfigReloadFailed?.(event.payload);
          },
        ),
      ]);
      const unlisteners = results.map((result) =>
        result.status === "fulfilled" ? result.value : null,
      );
      if (disposed) {
        for (const unlisten of unlisteners) unlisten?.();
        return;
      }
      [
        providersChangedUnlisten,
        eventsReconnectedUnlisten,
        storageBlockedUnlisten,
        configReloadedUnlisten,
        configReloadFailedUnlisten,
      ] = unlisteners;
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
    clearProviderFlushTimer();
    pendingProviderIds.clear();
    deferredProviderIds.clear();
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", flushDeferredProviderReload);
    }
    if (typeof window !== "undefined") {
      window.removeEventListener("focus", flushDeferredProviderReload);
    }
    options.cleanupThemeListener();
    options.resetProviderPointerDrag(false);
    providersChangedUnlisten?.();
    providersChangedUnlisten = null;
    eventsReconnectedUnlisten?.();
    eventsReconnectedUnlisten = null;
    storageBlockedUnlisten?.();
    storageBlockedUnlisten = null;
    configReloadedUnlisten?.();
    configReloadedUnlisten = null;
    configReloadFailedUnlisten?.();
    configReloadFailedUnlisten = null;
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
