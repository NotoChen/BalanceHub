import { computed, onUnmounted, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import {
  type CliConfigPreview,
  type CliConfigFile,
  type CliRuntimeSnapshot,
  type AgentCliKind,
  type Provider,
  type TemporaryCliInstance,
} from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment";

interface UseCliRuntimeOptions {
  providers: Ref<Provider[]>;
  cliRuntime: Ref<CliRuntimeSnapshot>;
  refreshInstances: () => Promise<TemporaryCliInstance[]>;
  activate: (instanceId: string) => Promise<void>;
  previewConfig: (providerId: string, cliKind: AgentCliKind) => Promise<CliConfigPreview>;
  switchConfig: (
    providerId: string,
    cliKind: AgentCliKind,
    revision: string,
    files: CliConfigFile[],
  ) => Promise<CliRuntimeSnapshot>;
}

export function useCliRuntime(options: UseCliRuntimeOptions) {
  const store = useCliRuntimeStore();
  const cliInstancesVisible = ref(false);
  const cliInstancesProviderId = ref<string | null>(null);
  const cliInstancesKind = ref<AgentCliKind | null>(null);
  const activatingCliInstanceId = ref<string | null>(null);
  const cliInstancesRefreshing = ref(false);
  const switchingCliConfig = ref<{ providerId: string; cliKind: AgentCliKind } | null>(null);
  const cliConfigPreviewVisible = ref(false);
  const cliConfigPreview = ref<CliConfigPreview | null>(null);
  let instanceRefreshPending = false;
  let instancePollTimer: number | null = null;

  const cliInstancesProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === cliInstancesProviderId.value,
    ) ?? null,
  );

  const cliInstances = computed(() =>
    options.cliRuntime.value.instances.filter(
      (instance) =>
        (!cliInstancesProviderId.value || instance.providerId === cliInstancesProviderId.value) &&
        (!cliInstancesKind.value || instance.cliKind === cliInstancesKind.value) &&
        instance.status !== "exited",
    ),
  );

  function openCliInstances(provider: Provider, cliKind: AgentCliKind) {
    cliInstancesProviderId.value = provider.identity.id;
    cliInstancesKind.value = cliKind;
    cliInstancesVisible.value = true;
    void refreshCliRuntime();
  }

  function openAgentCliInstances(kind: AgentCliKind) {
    cliInstancesProviderId.value = null;
    cliInstancesKind.value = kind;
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

  async function switchProviderCliConfig(provider: Provider, cliKind: AgentCliKind) {
    if (
      switchingCliConfig.value ||
      options.cliRuntime.value.configs.some(
        (snapshot) =>
          snapshot.cliKind === cliKind && snapshot.providerId === provider.identity.id,
      )
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

  async function confirmCliConfigSwitch(files?: CliConfigFile[]) {
    const preview = cliConfigPreview.value;
    if (!preview || switchingCliConfig.value || preview.files.length === 0) {
      return;
    }

    switchingCliConfig.value = {
      providerId: preview.providerId,
      cliKind: preview.cliKind,
    };
    try {
      await options.switchConfig(
        preview.providerId,
        preview.cliKind,
        preview.revision,
        files ?? preview.files,
      );
      cliConfigPreviewVisible.value = false;
      Message.success(
        `已将 ${preview.providerName} 设为 ${agentCliLabel(store.cliEnvironmentProbe, preview.cliKind)} 默认中转站`,
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
    cliInstancesKind,
    cliInstances,
    activatingCliInstanceId,
    cliInstancesRefreshing,
    switchingCliConfig,
    cliConfigPreviewVisible,
    cliConfigPreview,
    openCliInstances,
    openAgentCliInstances,
    refreshCliRuntime,
    activateCliInstance,
    switchProviderCliConfig,
    confirmCliConfigSwitch,
  };
}
