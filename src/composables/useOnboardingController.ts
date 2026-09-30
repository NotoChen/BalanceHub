import { computed, onScopeDispose, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";

interface UseOnboardingControllerOptions {
  initialized: Ref<boolean>;
  loadError: Ref<string | null>;
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  settingsForm: AppSettings;
  flushSettingsSave: () => Promise<boolean>;
  importAppData: () => Promise<unknown>;
  openAddProvider: () => void;
  openSettings: () => void;
}

export function useOnboardingController(options: UseOnboardingControllerOptions) {
  const hiddenForSession = ref(false);
  let completing = false;
  let disposed = false;
  onScopeDispose(() => { disposed = true; });

  const onboardingProviderCount = computed(() => options.providers.value.length);
  const onboardingCliConfigured = computed(() =>
    Boolean(
      Object.values(options.settings.value.agentCliPaths).some((path) => path?.trim()) ||
        Object.values(options.settingsForm.agentCliPaths).some((path) => path?.trim()),
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
    if (disposed || completing) return;
    completing = true;
    hiddenForSession.value = true;
    try {
      options.settingsForm.onboardingCompleted = true;
      if (!await options.flushSettingsSave() && !disposed) hiddenForSession.value = false;
    } catch (error) {
      if (!disposed) {
        Message.error(error instanceof Error ? error.message : String(error));
        hiddenForSession.value = false;
      }
    } finally {
      completing = false;
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
