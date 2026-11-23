import { computed, onScopeDispose, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { AppSettings, Provider } from "../stores/providers";
import type { CliEnvironmentProbeResult } from "../stores/provider-types";

interface UseOnboardingControllerOptions {
  initialized: Ref<boolean>;
  loadError: Ref<string | null>;
  providers: Ref<Provider[]>;
  settings: Ref<AppSettings>;
  settingsForm: AppSettings;
  cliEnvironmentProbe: Ref<CliEnvironmentProbeResult | null>;
  flushSettingsSave: () => Promise<boolean>;
  importAppData: () => Promise<unknown>;
  openAddProvider: () => void;
  detectCliTools: () => Promise<CliEnvironmentProbeResult>;
  openAgentWorkspace: () => void;
}

export function useOnboardingController(options: UseOnboardingControllerOptions) {
  const hiddenForSession = ref(false);
  const detectingCli = ref(false);
  let completing = false;
  let disposed = false;
  onScopeDispose(() => { disposed = true; });

  const onboardingProviderCount = computed(() => options.providers.value.length);
  const onboardingCliConfigured = computed(() =>
    Boolean(
      options.cliEnvironmentProbe.value?.tools.some((tool) => tool.available) ||
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

  async function detectOnboardingCliTools() {
    if (detectingCli.value) return;
    detectingCli.value = true;
    try {
      await options.detectCliTools();
    } catch (error) {
      if (!disposed) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      detectingCli.value = false;
    }
  }

  function openOnboardingAgentWorkspace() {
    hiddenForSession.value = true;
    options.openAgentWorkspace();
  }

  async function importOnboardingData() {
    await options.importAppData();
  }

  /// 关闭窗口只隐藏本次会话，不写 onboardingCompleted；只有「进入应用」才
  /// 代表用户确认完成初始化。
  function dismissOnboarding() {
    hiddenForSession.value = true;
  }

  async function completeOnboarding() {
    if (disposed || completing) return;
    completing = true;
    hiddenForSession.value = true;
    const previousCompleted = options.settingsForm.onboardingCompleted;
    try {
      options.settingsForm.onboardingCompleted = true;
      if (!await options.flushSettingsSave() && !disposed) {
        // 保存没有确认时把草稿恢复原值：否则后续任意一次设置保存都会把这
        // 次未完成的「进入应用」一并写盘，和「关闭只是本次隐藏」冲突。
        options.settingsForm.onboardingCompleted = previousCompleted;
        hiddenForSession.value = false;
      }
    } catch (error) {
      if (!disposed) {
        options.settingsForm.onboardingCompleted = previousCompleted;
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
    detectingOnboardingCli: detectingCli,
    openOnboardingAddProvider,
    detectOnboardingCliTools,
    openOnboardingAgentWorkspace,
    importOnboardingData,
    dismissOnboarding,
    completeOnboarding,
  };
}
