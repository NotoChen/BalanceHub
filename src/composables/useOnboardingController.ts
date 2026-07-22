import { computed, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";

interface UseOnboardingControllerOptions {
  initialized: Ref<boolean>;
  loadError: Ref<string | null>;
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  settingsForm: AppSettings;
  saveSettings: (settings: AppSettings) => Promise<unknown>;
  syncFromSettings: (settings?: AppSettings) => void;
  importAppData: () => Promise<unknown>;
  openAddProvider: () => void;
  openSettings: () => void;
}

export function useOnboardingController(options: UseOnboardingControllerOptions) {
  const hiddenForSession = ref(false);

  const onboardingProviderCount = computed(() => options.providers.value.length);
  const onboardingCliConfigured = computed(() =>
    Boolean(
      options.settings.value.codexCliPath.trim() ||
        options.settings.value.claudeCliPath.trim() ||
        options.settingsForm.codexCliPath.trim() ||
        options.settingsForm.claudeCliPath.trim(),
    ),
  );
  const onboardingVisible = computed(
    () =>
      options.initialized.value &&
      !options.loadError.value &&
      !hiddenForSession.value &&
      !options.settings.value.onboardingCompleted &&
      onboardingProviderCount.value === 0,
  );

  function openOnboardingAddProvider() {
    hiddenForSession.value = true;
    options.openAddProvider();
  }

  function openOnboardingSettings() {
    hiddenForSession.value = true;
    options.openSettings();
  }

  async function importOnboardingData() {
    await options.importAppData();
  }

  async function completeOnboarding() {
    hiddenForSession.value = true;
    try {
      const nextSettings = {
        ...options.settings.value,
        onboardingCompleted: true,
      };
      await options.saveSettings(nextSettings);
      options.syncFromSettings(nextSettings);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
      hiddenForSession.value = false;
    }
  }

  return {
    onboardingVisible,
    onboardingProviderCount,
    onboardingCliConfigured,
    openOnboardingAddProvider,
    openOnboardingSettings,
    importOnboardingData,
    completeOnboarding,
  };
}
