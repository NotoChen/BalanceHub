import { computed, onBeforeUnmount, reactive, ref, watch, type Ref } from "vue";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { isWebHost } from "../api/transport.ts";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import { useThemeMode } from "./useThemeMode";
import { defaultSettings } from "../stores/providers";
import { providerDisplayLabel } from "../utils/provider-display";
import { createSettingsSaveQueue, type SettingsSaveState } from "../utils/settings-save-queue";

interface UseSettingsControllerOptions {
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  initialSettings: AppSettings;
  saveSettings: (settings: AppSettings, expected: AppSettings) => Promise<AppSettings>;
}

export type { SettingsSaveState } from "../utils/settings-save-queue";

const MAX_LIVENESS_MODEL_OPTIONS = 2_000;

export function useSettingsController(options: UseSettingsControllerOptions) {
  const settingsDrawerVisible = ref(false);
  const settingsForm = reactive(cloneSettings(options.initialSettings));
  const settingsSaveState = ref<SettingsSaveState>("saved");
  const settingsSaveError = ref("");
  const { applyTheme, setupThemeListener, cleanupThemeListener } = useThemeMode(settingsForm);

  let lastLaunchAtLogin = settingsForm.launchAtLogin;
  /// 后端快照最近一次进入表单的设置内容。`providers-changed` 很频繁，
  /// 内容没变时不需要重复改写草稿。
  let lastHydratedSettings = JSON.stringify(options.settings.value);
  /// 外部 config.json 热重载后，下一次设置水合需要把开机启动同步到新配置。
  /// 不能立刻读取宿主状态——那是反方向，会把用户刚改的配置覆盖掉。
  let pendingExternalLaunchSync = false;
  const saveQueue = createSettingsSaveQueue({
    read: () => {
      const draft = cloneSettings(settingsForm);
      normalizeLivenessTiming(draft);
      return draft;
    },
    write: async (payload, expected) => {
      // 浏览器版没有登录自启动；跳过系统调用，只保存设置本身。
      if (!isWebHost && lastLaunchAtLogin !== payload.launchAtLogin) {
        if (payload.launchAtLogin) await enable();
        else await disable();
        lastLaunchAtLogin = payload.launchAtLogin;
      }
      return options.saveSettings(payload, expected);
    },
    accept: (saved) => { Object.assign(settingsForm, cloneSettings(saved)); },
    state: (state, error) => {
      settingsSaveState.value = state;
      settingsSaveError.value = error;
    },
    failed: (message) => { Message.error(`应用设置未保存：${message}`); },
  });

  const livenessModelOptions = computed(() => {
    const models = new Set<string>();
    outer: for (const provider of options.providers.value) {
      for (const rawModel of provider.capabilities.availableModels || []) {
        const model = rawModel.trim();
        if (model) {
          models.add(model);
        }
        if (models.size >= MAX_LIVENESS_MODEL_OPTIONS) {
          break outer;
        }
      }
    }
    return Array.from(models).sort();
  });

  const selectedLivenessModelProviders = computed(() => {
    const selectedModel = settingsForm.livenessModel.trim();
    if (!selectedModel) return [];
    return options.providers.value
      .filter((provider) =>
        (provider.capabilities.availableModels || []).some(
          (model) => model.trim() === selectedModel,
        ),
      )
      .map((provider) => ({ id: provider.identity.id, name: providerDisplayLabel(provider) }))
      .sort((left, right) => left.name.localeCompare(right.name));
  });

  async function syncLaunchAtLogin() {
    if (isWebHost) {
      settingsForm.launchAtLogin = options.settings.value.launchAtLogin;
      return;
    }
    try {
      settingsForm.launchAtLogin = await isEnabled();
    } catch {
      settingsForm.launchAtLogin = options.settings.value.launchAtLogin;
    }
  }

  /// 按配置值调整系统开机启动项；失败时回读宿主真实状态，避免界面显示
  /// 一个并未生效的开关。成功/失败后 saveQueue 写入都会以宿主为准。
  async function applyLaunchAtLogin(desired: boolean) {
    if (isWebHost) return;
    try {
      if (desired) await enable();
      else await disable();
      lastLaunchAtLogin = desired;
    } catch (error) {
      Message.error(
        `开机启动设置未应用：${error instanceof Error ? error.message : String(error)}`,
      );
      try {
        settingsForm.launchAtLogin = await isEnabled();
        lastLaunchAtLogin = settingsForm.launchAtLogin;
      } catch {
        // 连读取都失败时保持当前草稿，后续保存仍会重试。
      }
    }
  }

  function requestExternalLaunchSync() {
    pendingExternalLaunchSync = true;
  }

  function syncFromSettings(value = options.settings.value) {
    // 用户有未保存草稿时宿主以草稿为准，外部配置的开机启动同步随之作废，
    // 不能等到下一次普通保存后再反过来改系统设置。
    const externalLaunchSync = pendingExternalLaunchSync;
    pendingExternalLaunchSync = false;
    if (!saveQueue.acceptExternal(value)) return false;
    Object.assign(settingsForm, cloneSettings(value));
    lastHydratedSettings = JSON.stringify(value);
    const desired = settingsForm.launchAtLogin;
    // 先记账再异步应用，避免 settingsForm 的保存队列重复调用 enable/disable。
    const applyToHost = externalLaunchSync && lastLaunchAtLogin !== desired;
    lastLaunchAtLogin = desired;
    if (applyToHost) void applyLaunchAtLogin(desired);
    applyTheme(value.themeMode);
    return true;
  }

  watch(
    options.settings,
    (value) => {
      // 每次后端快照都会换新对象；内容没变时不必重写草稿。接受失败说明
      // 本地还有未保存草稿，保留旧标记，等下一次快照重试。
      if (JSON.stringify(value) === lastHydratedSettings) return;
      syncFromSettings(value);
    },
  );

  watch(
    settingsForm,
    () => {
      applyTheme(settingsForm.themeMode);
      saveQueue.schedule();
    },
    { deep: true },
  );

  onBeforeUnmount(saveQueue.dispose);

  return {
    settingsDrawerVisible,
    settingsSaveState,
    settingsSaveError,
    settingsForm,
    livenessModelOptions,
    selectedLivenessModelProviders,
    applyTheme,
    setupThemeListener,
    cleanupThemeListener,
    flushSettingsSave: saveQueue.flush,
    replaceSettings: <R>(operation: () => Promise<R>) => saveQueue.replace(async () => ({
      result: await operation(),
      settings: cloneSettings(options.settings.value),
    })),
    syncLaunchAtLogin,
    requestExternalLaunchSync,
    syncFromSettings,
  };
}

function cloneSettings(settings: AppSettings): AppSettings {
  return {
    ...defaultSettings(),
    ...JSON.parse(JSON.stringify(settings)),
  };
}
