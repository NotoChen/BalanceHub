import { computed, onBeforeUnmount, reactive, ref, watch, type Ref } from "vue";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
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
  saveSettings: (settings: AppSettings) => Promise<AppSettings>;
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
  const saveQueue = createSettingsSaveQueue({
    read: () => {
      const draft = cloneSettings(settingsForm);
      normalizeLivenessTiming(draft);
      return draft;
    },
    write: async (payload) => {
      if (lastLaunchAtLogin !== payload.launchAtLogin) {
        if (payload.launchAtLogin) await enable();
        else await disable();
        lastLaunchAtLogin = payload.launchAtLogin;
      }
      return options.saveSettings(payload);
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
    try {
      settingsForm.launchAtLogin = await isEnabled();
    } catch {
      settingsForm.launchAtLogin = options.settings.value.launchAtLogin;
    }
  }

  function syncFromSettings(value = options.settings.value) {
    if (!saveQueue.acceptExternal(value)) return;
    Object.assign(settingsForm, cloneSettings(value));
    lastLaunchAtLogin = settingsForm.launchAtLogin;
    applyTheme(value.themeMode);
  }

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
    syncFromSettings,
  };
}

function cloneSettings(settings: AppSettings): AppSettings {
  return {
    ...defaultSettings(),
    ...JSON.parse(JSON.stringify(settings)),
  };
}
