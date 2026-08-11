import { computed, onBeforeUnmount, reactive, ref, watch, type Ref } from "vue";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, CliEnvironmentProbeResult, Provider } from "../stores/providers";
import { durationValueToSeconds, secondsToDurationValue, type DurationUnit } from "../utils/duration";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import {
  applyCliEnvironmentProbeResult,
  captureCliEnvironmentSettings,
} from "../utils/cli-environment";
import { useThemeMode } from "./useThemeMode";
import { defaultSettings } from "../stores/providers";

interface UseSettingsControllerOptions {
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  initialSettings: AppSettings;
  saveSettings: (settings: AppSettings) => Promise<unknown>;
  probeCliTools: (deep?: boolean) => Promise<CliEnvironmentProbeResult>;
}

export type SettingsSaveState = "saved" | "pending" | "saving" | "error";

const MAX_LIVENESS_MODEL_OPTIONS = 2_000;

export function useSettingsController(options: UseSettingsControllerOptions) {
  const settingsDrawerVisible = ref(false);
  const probingCliEnvironment = ref(false);
  const settingsForm = reactive(cloneSettings(options.initialSettings));
  const settingsSaveState = ref<SettingsSaveState>("saved");
  const globalRefreshUnit = ref<DurationUnit>("minute");
  const { applyTheme, setupThemeListener, cleanupThemeListener } = useThemeMode(settingsForm);

  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let activeSave: Promise<void> | null = null;
  let queuedSave = false;
  let disposed = false;
  let lastPersistedSnapshot = settingsSnapshot(settingsForm);
  let lastLaunchAtLogin = settingsForm.launchAtLogin;

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
      .map((provider) => ({ id: provider.identity.id, name: provider.identity.name }))
      .sort((left, right) => left.name.localeCompare(right.name));
  });

  const globalRefreshAmount = computed({
    get: () => secondsToDurationValue(settingsForm.refreshInterval, globalRefreshUnit.value),
    set: (value: number | undefined) => {
      settingsForm.refreshInterval = Math.max(30, durationValueToSeconds(value, globalRefreshUnit.value));
    },
  });

  function scheduleSettingsSave() {
    if (disposed) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = null;
      void persistSettings();
    }, 300);
  }

  async function persistSettings(): Promise<void> {
    if (activeSave) {
      queuedSave = true;
      return activeSave;
    }

    const task = (async () => {
      do {
        queuedSave = false;
        normalizeLivenessTiming(settingsForm);
        const payload = cloneSettings(settingsForm);
        const snapshot = settingsSnapshot(payload);
        settingsSaveState.value = "saving";

        try {
          if (lastLaunchAtLogin !== payload.launchAtLogin) {
            if (payload.launchAtLogin) {
              await enable();
            } else {
              await disable();
            }
            lastLaunchAtLogin = payload.launchAtLogin;
          }
          await options.saveSettings(payload);
          if (!disposed && settingsSnapshot(settingsForm) !== snapshot) {
            queuedSave = true;
          } else {
            lastPersistedSnapshot = snapshot;
            settingsSaveState.value = "saved";
          }
        } catch (error) {
          settingsSaveState.value = "error";
          if (!disposed) {
            Message.error(error instanceof Error ? error.message : String(error));
          }
        }
      } while (!disposed && queuedSave && settingsSaveState.value !== "error");
    })();

    activeSave = task;
    try {
      await task;
    } finally {
      activeSave = null;
    }
  }

  async function flushSettingsSave() {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    if (activeSave) {
      await activeSave;
    }
    if (settingsSnapshot(settingsForm) !== lastPersistedSnapshot) {
      await persistSettings();
    }
  }

  async function probeCliTools() {
    if (probingCliEnvironment.value) {
      return;
    }

    const settingsAtStart = captureCliEnvironmentSettings(settingsForm);
    probingCliEnvironment.value = true;
    try {
      const result = await options.probeCliTools(true);
      applyCliEnvironmentProbeResult(settingsForm, result, settingsAtStart);
    } catch (error) {
      // 自动探测失败只在设置卡片内呈现，不打断启动流程。
      if (settingsDrawerVisible.value) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      probingCliEnvironment.value = false;
    }
  }

  async function autoProbeCliTools() {
    try {
      await options.probeCliTools(false);
    } catch {
      // Keep startup quiet; the settings panel presents the unavailable state.
    }
  }

  async function syncLaunchAtLogin() {
    try {
      settingsForm.launchAtLogin = await isEnabled();
    } catch {
      settingsForm.launchAtLogin = options.settings.value.launchAtLogin;
    }
  }

  function syncFromSettings(value = options.settings.value) {
    Object.assign(settingsForm, cloneSettings(value));
    lastPersistedSnapshot = settingsSnapshot(settingsForm);
    lastLaunchAtLogin = settingsForm.launchAtLogin;
    settingsSaveState.value = "saved";
    applyTheme(value.themeMode);
  }

  async function resetDraftOnClose() {
    await flushSettingsSave();
    Object.assign(settingsForm, cloneSettings(options.settings.value));
    lastPersistedSnapshot = settingsSnapshot(settingsForm);
    lastLaunchAtLogin = settingsForm.launchAtLogin;
    settingsSaveState.value = "saved";
    applyTheme(options.settings.value.themeMode);
  }

  watch(
    settingsForm,
    () => {
      applyTheme(settingsForm.themeMode);
      if (settingsSnapshot(settingsForm) === lastPersistedSnapshot) return;
      settingsSaveState.value = "pending";
      scheduleSettingsSave();
    },
    { deep: true },
  );

  onBeforeUnmount(() => {
    disposed = true;
    queuedSave = false;
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
  });

  return {
    settingsDrawerVisible,
    settingsSaveState,
    probingCliEnvironment,
    settingsForm,
    globalRefreshUnit,
    livenessModelOptions,
    selectedLivenessModelProviders,
    globalRefreshAmount,
    applyTheme,
    setupThemeListener,
    cleanupThemeListener,
    flushSettingsSave,
    probeCliTools,
    autoProbeCliTools,
    syncLaunchAtLogin,
    syncFromSettings,
    resetDraftOnClose,
  };
}

function cloneSettings(settings: AppSettings): AppSettings {
  return {
    ...defaultSettings(),
    ...JSON.parse(JSON.stringify(settings)),
  };
}

function settingsSnapshot(settings: AppSettings) {
  return JSON.stringify(settings);
}
