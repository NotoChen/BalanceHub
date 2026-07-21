import { computed, reactive, ref, type Ref } from "vue";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";
import type { CodexCliProbeInput } from "../api/app";
import { durationValueToSeconds, secondsToDurationValue, type DurationUnit } from "../utils/duration";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import { useThemeMode } from "./useThemeMode";
import { defaultSettings } from "../stores/providers";

interface UseSettingsControllerOptions {
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  initialSettings: AppSettings;
  saveSettings: (settings: AppSettings) => Promise<unknown>;
  probeCodexCli: (input?: Partial<CodexCliProbeInput>) => Promise<{ path: string; version: string }>;
}

export function useSettingsController(options: UseSettingsControllerOptions) {
  const settingsDrawerVisible = ref(false);
  const probingCodexCliPath = ref(false);
  const settingsForm = reactive(cloneSettings(options.initialSettings));
  const globalRefreshUnit = ref<DurationUnit>("minute");
  const { applyTheme, setupThemeListener, cleanupThemeListener } = useThemeMode(settingsForm);

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

  async function saveSettings() {
    normalizeLivenessTiming(settingsForm);
    if (settingsForm.launchAtLogin) {
      await enable();
    } else {
      await disable();
    }
    await options.saveSettings(cloneSettings(settingsForm));
    applyTheme(settingsForm.themeMode);
    settingsDrawerVisible.value = false;
  }

  async function probeCodexCliPath() {
    if (probingCodexCliPath.value) {
      return;
    }

    probingCodexCliPath.value = true;
    try {
      const result = await options.probeCodexCli({
        livenessCliKind: settingsForm.livenessCliKind,
        codexCliPath: settingsForm.codexCliPath,
        claudeCliPath: settingsForm.claudeCliPath,
      });
      if (settingsForm.livenessCliKind === "claudeCode") {
        settingsForm.claudeCliPath = result.path;
      } else {
        settingsForm.codexCliPath = result.path;
      }
      Message.success(`已找到测活 CLI：${result.version || result.path}`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      probingCodexCliPath.value = false;
    }
  }

  async function autoProbeCodexCliPath() {
    try {
      await options.probeCodexCli();
      Object.assign(settingsForm, cloneSettings(options.settings.value));
    } catch {
      // Keep startup quiet; the settings panel still exposes manual path configuration.
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
    applyTheme(value.themeMode);
  }

  function resetDraftOnClose() {
    Object.assign(settingsForm, cloneSettings(options.settings.value));
    applyTheme(options.settings.value.themeMode);
  }

  return {
    settingsDrawerVisible,
    probingCodexCliPath,
    settingsForm,
    globalRefreshUnit,
    livenessModelOptions,
    modelProviderIndex,
    globalRefreshAmount,
    applyTheme,
    setupThemeListener,
    cleanupThemeListener,
    saveSettings,
    probeCodexCliPath,
    autoProbeCodexCliPath,
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
