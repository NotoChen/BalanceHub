import { computed, onUnmounted, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import {
  type CliConfigPreview,
  type CliConfigFile,
  type CliRuntimeSnapshot,
  type AgentCliKind,
  type Provider,
  type ProviderApiKeyOption,
  type TemporaryCliInstance,
} from "../stores/providers";
import { agentCliLabel } from "../utils/cli-environment";
import { withTimeout } from "../utils/promise-timeout";
import { providerDisplayLabel } from "../utils/provider-display";
import {
  effectiveProviderApiKeyOptions,
  isProviderApiKeyUsable,
} from "../utils/provider-api-key-options";

const CLI_RUNTIME_REFRESH_TIMEOUT_MS = 15_000;
const CLI_CONFIG_PREVIEW_TIMEOUT_MS = 30_000;
const CLI_CONFIG_SWITCH_TIMEOUT_MS = 30_000;
const CLI_ACTIVATION_TIMEOUT_MS = 15_000;

interface UseCliRuntimeOptions {
  providers: Ref<Provider[]>;
  cliRuntime: Ref<CliRuntimeSnapshot>;
  refreshInstances: () => Promise<TemporaryCliInstance[]>;
  activate: (instanceId: string) => Promise<void>;
  previewConfig: (
    providerId: string,
    cliKind: AgentCliKind,
    apiKeyLocalId: string,
  ) => Promise<CliConfigPreview>;
  switchConfig: (
    providerId: string,
    cliKind: AgentCliKind,
    apiKeyLocalId: string,
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
  const cliConfigKeyPickerVisible = ref(false);
  const cliConfigKeyPickerProvider = ref<Provider | null>(null);
  const cliConfigKeyPickerKind = ref<AgentCliKind | null>(null);
  const cliConfigKeyPickerKeys = ref<ProviderApiKeyOption[]>([]);
  const cliConfigPreviewVisible = ref(false);
  const cliConfigPreview = ref<CliConfigPreview | null>(null);
  let instanceRefreshPending = false;
  let instancePollTimer: number | null = null;
  let cliConfigRequestRevision = 0;

  watch(cliConfigKeyPickerVisible, (visible) => {
    if (visible || cliConfigPreviewVisible.value) return;
    // Closing the key picker while a preview request is pending must make the
    // eventual response stale; otherwise a late IPC result can reopen the
    // configuration editor after the user explicitly cancelled.
    cliConfigRequestRevision += 1;
    switchingCliConfig.value = null;
    cliConfigKeyPickerProvider.value = null;
    cliConfigKeyPickerKind.value = null;
    cliConfigKeyPickerKeys.value = [];
  });

  watch(cliConfigPreviewVisible, (visible) => {
    if (visible || switchingCliConfig.value) return;
    cliConfigRequestRevision += 1;
    cliConfigPreview.value = null;
  });

  const cliInstancesProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === cliInstancesProviderId.value,
    ) ?? null,
  );

  const cliInstances = computed(() => {
    const providerLabels = new Map(
      options.providers.value.map((provider) => [provider.identity.id, providerDisplayLabel(provider)]),
    );
    return options.cliRuntime.value.instances
      .filter(
        (instance) =>
          (!cliInstancesProviderId.value || instance.providerId === cliInstancesProviderId.value) &&
          (!cliInstancesKind.value || instance.cliKind === cliInstancesKind.value) &&
          instance.status !== "exited",
      )
      .map((instance) => ({
        ...instance,
        providerName: providerLabels.get(instance.providerId) || instance.providerName,
      }));
  });

  const cliConfigKeyPickerCurrentConfig = computed(() => {
    const provider = cliConfigKeyPickerProvider.value;
    const cliKind = cliConfigKeyPickerKind.value;
    if (!provider || !cliKind) return null;
    return options.cliRuntime.value.configs.find(
      (snapshot) =>
        snapshot.cliKind === cliKind && snapshot.providerId === provider.identity.id,
    ) ?? null;
  });

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
      await withTimeout(
        options.refreshInstances(),
        CLI_RUNTIME_REFRESH_TIMEOUT_MS,
        "读取临时 CLI 状态超时",
      );
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
    if (switchingCliConfig.value) {
      return;
    }

    const keys = effectiveProviderApiKeyOptions(
      provider.auth.apiKey,
      provider.auth.apiKeyOptions || [],
    ).filter(isProviderApiKeyUsable);
    if (keys.length === 0) {
      Message.warning("当前中转站没有可用于 Agent 默认配置的完整 API Key");
      return;
    }
    if (keys.length === 1) {
      await previewProviderCliConfig(provider, cliKind, keys[0]);
      return;
    }

    cliConfigKeyPickerProvider.value = provider;
    cliConfigKeyPickerKind.value = cliKind;
    cliConfigKeyPickerKeys.value = keys;
    cliConfigKeyPickerVisible.value = true;
  }

  async function selectCliConfigApiKey(option: ProviderApiKeyOption) {
    const provider = cliConfigKeyPickerProvider.value;
    const cliKind = cliConfigKeyPickerKind.value;
    if (!provider || !cliKind || switchingCliConfig.value) return;
    await previewProviderCliConfig(provider, cliKind, option);
    if (cliConfigPreviewVisible.value) {
      cliConfigKeyPickerVisible.value = false;
    }
  }

  async function previewProviderCliConfig(
    provider: Provider,
    cliKind: AgentCliKind,
    apiKey: ProviderApiKeyOption,
  ) {
    const requestRevision = ++cliConfigRequestRevision;
    switchingCliConfig.value = { providerId: provider.identity.id, cliKind };
    try {
      const preview = await withTimeout(
        options.previewConfig(provider.identity.id, cliKind, apiKey.localId.trim()),
        CLI_CONFIG_PREVIEW_TIMEOUT_MS,
        "读取 CLI 配置预览超时",
      );
      if (requestRevision !== cliConfigRequestRevision) return;
      cliConfigPreview.value = preview;
      cliConfigPreviewVisible.value = true;
    } catch (error) {
      if (requestRevision === cliConfigRequestRevision) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (requestRevision === cliConfigRequestRevision) {
        switchingCliConfig.value = null;
      }
    }
  }

  function confirmCliConfigSwitch(files?: CliConfigFile[]) {
    const preview = cliConfigPreview.value;
    if (!preview || switchingCliConfig.value || preview.files.length === 0) {
      return;
    }

    const requestRevision = ++cliConfigRequestRevision;
    switchingCliConfig.value = {
      providerId: preview.providerId,
      cliKind: preview.cliKind,
    };
    cliConfigPreviewVisible.value = false;
    void switchCliConfigInBackground(preview, files ?? preview.files, requestRevision);
  }

  async function switchCliConfigInBackground(
    preview: CliConfigPreview,
    files: CliConfigFile[],
    requestRevision: number,
  ) {
    try {
      const runtime = await withTimeout(
        options.switchConfig(
          preview.providerId,
          preview.cliKind,
          preview.apiKeyLocalId,
          preview.revision,
          files,
        ),
        CLI_CONFIG_SWITCH_TIMEOUT_MS,
        "保存 CLI 默认配置超时",
      );
      if (requestRevision === cliConfigRequestRevision) {
        store.cliRuntime = runtime;
        Message.success(
          `已将 ${preview.providerName} · ${preview.apiKeyLabel} 设为 ${agentCliLabel(store.cliEnvironmentProbe, preview.cliKind)} 默认配置`,
        );
      }
    } catch (error) {
      if (requestRevision === cliConfigRequestRevision) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (requestRevision === cliConfigRequestRevision) {
        switchingCliConfig.value = null;
      }
    }
  }

  async function activateCliInstance(instance: TemporaryCliInstance) {
    activatingCliInstanceId.value = instance.id;
    try {
      await withTimeout(
        options.activate(instance.id),
        CLI_ACTIVATION_TIMEOUT_MS,
        "激活临时 CLI 终端超时",
      );
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
    cliConfigKeyPickerVisible,
    cliConfigKeyPickerProvider,
    cliConfigKeyPickerKind,
    cliConfigKeyPickerKeys,
    cliConfigKeyPickerCurrentConfig,
    cliConfigPreviewVisible,
    cliConfigPreview,
    openCliInstances,
    openAgentCliInstances,
    refreshCliRuntime,
    activateCliInstance,
    switchProviderCliConfig,
    selectCliConfigApiKey,
    confirmCliConfigSwitch,
  };
}
