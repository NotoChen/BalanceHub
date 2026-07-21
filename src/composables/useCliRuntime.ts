import { computed, onUnmounted, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  CliConfigPreview,
  CliRuntimeSnapshot,
  LivenessCliKind,
  Provider,
  TemporaryCliInstance,
} from "../stores/providers";

interface UseCliRuntimeOptions {
  providers: Ref<Provider[]>;
  cliRuntime: Ref<CliRuntimeSnapshot>;
  refreshInstances: () => Promise<TemporaryCliInstance[]>;
  activate: (instanceId: string) => Promise<void>;
  previewConfig: (providerId: string, cliKind: LivenessCliKind) => Promise<CliConfigPreview>;
  switchConfig: (
    providerId: string,
    cliKind: LivenessCliKind,
    revision: string,
  ) => Promise<CliRuntimeSnapshot>;
}

export function useCliRuntime(options: UseCliRuntimeOptions) {
  const cliInstancesVisible = ref(false);
  const cliInstancesProviderId = ref<string | null>(null);
  const activatingCliInstanceId = ref<string | null>(null);
  const cliInstancesRefreshing = ref(false);
  const switchingCliConfig = ref<{ providerId: string; cliKind: LivenessCliKind } | null>(null);
  const cliConfigPreviewVisible = ref(false);
  const cliConfigPreview = ref<CliConfigPreview | null>(null);
  let instanceRefreshPending = false;
  let instancePollTimer: number | null = null;

  const cliInstancesProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === cliInstancesProviderId.value,
    ) ?? null,
  );

  const providerCliInstances = computed(() =>
    options.cliRuntime.value.instances.filter(
      (instance) =>
        instance.providerId === cliInstancesProviderId.value && instance.status !== "exited",
    ),
  );

  function openCliInstances(provider: Provider) {
    cliInstancesProviderId.value = provider.identity.id;
    cliInstancesVisible.value = true;
    void refreshCliRuntime();
  }

  async function refreshCliRuntime(silent = false) {
    if (instanceRefreshPending) {
      return;
    }
    instanceRefreshPending = true;
    if (!silent) {
      cliInstancesRefreshing.value = true;
    }
    try {
      await options.refreshInstances();
    } catch (error) {
      if (!silent) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      instanceRefreshPending = false;
      if (!silent) {
        cliInstancesRefreshing.value = false;
      }
    }
  }

  function stopInstancePolling() {
    if (instancePollTimer !== null) {
      window.clearInterval(instancePollTimer);
      instancePollTimer = null;
    }
  }

  watch(
    () => options.cliRuntime.value.instances.length,
    (count) => {
      if (count === 0) {
        stopInstancePolling();
      } else if (instancePollTimer === null) {
        instancePollTimer = window.setInterval(() => {
          void refreshCliRuntime(true);
        }, 4_000);
      }
    },
    { immediate: true },
  );

  onUnmounted(stopInstancePolling);

  async function switchProviderCliConfig(provider: Provider, cliKind: LivenessCliKind) {
    if (
      switchingCliConfig.value ||
      options.cliRuntime.value[cliKind].providerId === provider.identity.id
    ) {
      return;
    }

    switchingCliConfig.value = { providerId: provider.identity.id, cliKind };
    try {
      cliConfigPreview.value = await options.previewConfig(provider.identity.id, cliKind);
      cliConfigPreviewVisible.value = true;
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      switchingCliConfig.value = null;
    }
  }

  async function confirmCliConfigSwitch() {
    const preview = cliConfigPreview.value;
    if (!preview || switchingCliConfig.value || preview.changes.length === 0) {
      return;
    }

    switchingCliConfig.value = {
      providerId: preview.providerId,
      cliKind: preview.cliKind,
    };
    try {
      await options.switchConfig(preview.providerId, preview.cliKind, preview.revision);
      cliConfigPreviewVisible.value = false;
      Message.success(
        `已将 ${preview.providerName} 设为 ${preview.cliKind === "codex" ? "Codex" : "Claude Code"} 默认中转站`,
      );
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      switchingCliConfig.value = null;
    }
  }

  async function activateCliInstance(instance: TemporaryCliInstance) {
    activatingCliInstanceId.value = instance.id;
    try {
      await options.activate(instance.id);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      activatingCliInstanceId.value = null;
    }
  }

  return {
    cliInstancesVisible,
    cliInstancesProvider,
    providerCliInstances,
    activatingCliInstanceId,
    cliInstancesRefreshing,
    switchingCliConfig,
    cliConfigPreviewVisible,
    cliConfigPreview,
    openCliInstances,
    refreshCliRuntime,
    activateCliInstance,
    switchProviderCliConfig,
    confirmCliConfigSwitch,
  };
}
