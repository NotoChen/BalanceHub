import { computed, reactive, ref, watch, type Ref } from "vue";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, CliEnvironmentProbeResult, Provider } from "../stores/providers";
import { durationValueToSeconds, secondsToDurationValue, type DurationUnit } from "../utils/duration";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import { useThemeMode } from "./useThemeMode";
import { defaultSettings } from "../stores/providers";

interface UseSettingsControllerOptions {
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  initialSettings: AppSettings;
  saveSettings: (settings: AppSettings) => Promise<unknown>;
  probeCliEnvironment: (
    terminalKind?: AppSettings["temporaryCliTerminalKind"],
    terminalCommand?: string,
  ) => Promise<CliEnvironmentProbeResult>;
}

export type SettingsSaveState = "saved" | "pending" | "saving" | "error";

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
  let lastPersistedSnapshot = settingsSnapshot(settingsForm);
  let lastLaunchAtLogin = settingsForm.launchAtLogin;

  const livenessModelOptions = computed(() =>
    Array.from(
      new Set(
        options.providers.value.flatMap((provider) =>
          (provider.capabilities.availableModels || []).map((model) => model.trim()).filter(Boolean),
        ),
      ),
    ).sort(),
  );

  const modelProviderIndex = computed(() => {
    const index = new Map<string, { model: string; providers: { id: string; name: string }[] }>();
    for (const provider of options.providers.value) {
      for (const rawModel of provider.capabilities.availableModels || []) {
        const model = rawModel.trim();
        if (!model) continue;
        const item = index.get(model) ?? { model, providers: [] };
        item.providers.push({ id: provider.identity.id, name: provider.identity.name });
        index.set(model, item);
      }
    }
    return Array.from(index.values())
      .map((item) => ({
        ...item,
        providers: item.providers.sort((a, b) => a.name.localeCompare(b.name)),
      }))
      .sort((a, b) => a.model.localeCompare(b.model));
  });

  const globalRefreshAmount = computed({
    get: () => secondsToDurationValue(settingsForm.refreshInterval, globalRefreshUnit.value),
    set: (value: number | undefined) => {
      settingsForm.refreshInterval = Math.max(30, durationValueToSeconds(value, globalRefreshUnit.value));
    },
  });

  function scheduleSettingsSave() {
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
          if (settingsSnapshot(settingsForm) !== snapshot) {
            queuedSave = true;
          } else {
            lastPersistedSnapshot = snapshot;
            settingsSaveState.value = "saved";
          }
        } catch (error) {
          settingsSaveState.value = "error";
          Message.error(error instanceof Error ? error.message : String(error));
        }
      } while (queuedSave && settingsSaveState.value !== "error");
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

  async function probeCliEnvironment() {
    if (probingCliEnvironment.value) {
      return;
    }

    probingCliEnvironment.value = true;
    try {
      const result = await options.probeCliEnvironment(
        settingsForm.temporaryCliTerminalKind,
        settingsForm.temporaryCliTerminalCommand,
      );
      settingsForm.codexCliPath = result.codex.path;
      settingsForm.claudeCliPath = result.claudeCode.path;
    } catch (error) {
      // 自动探测失败只在设置卡片内呈现，不打断启动流程。
      if (settingsDrawerVisible.value) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      probingCliEnvironment.value = false;
    }
  }

  async function autoProbeCliEnvironment() {
    try {
      const result = await options.probeCliEnvironment(
        settingsForm.temporaryCliTerminalKind,
        settingsForm.temporaryCliTerminalCommand,
      );
      settingsForm.codexCliPath = result.codex.path;
      settingsForm.claudeCliPath = result.claudeCode.path;
      Object.assign(settingsForm, cloneSettings(options.settings.value));
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

  return {
    settingsDrawerVisible,
    settingsSaveState,
    probingCliEnvironment,
    settingsForm,
    globalRefreshUnit,
    livenessModelOptions,
    modelProviderIndex,
    globalRefreshAmount,
    applyTheme,
    setupThemeListener,
    cleanupThemeListener,
    flushSettingsSave,
    probeCliEnvironment,
    autoProbeCliEnvironment,
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
