import { computed, onMounted, onUnmounted, reactive, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { listen } from "@tauri-apps/api/event";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useAgentConfigurationEditor } from "./useAgentConfigurationEditor";
import type { AgentConfigurationEdit } from "../stores/agent-configuration-types";
import {
  type CliRuntimeSnapshot,
  type AgentCliKind,
  type AgentRuntimeSession,
  type AgentRuntimeSnapshot,
  type Provider,
  type ProviderApiKeyOption,
} from "../stores/providers";
import { activeAgentRuntimeSessions } from "../utils/agent-runtime";
import { agentCliLabel } from "../utils/cli-environment";
import { withTimeout } from "../utils/promise-timeout";
import { providerDisplayLabel } from "../utils/provider-display";
import {
  effectiveProviderApiKeyOptions,
  isProviderApiKeyUsable,
} from "../utils/provider-api-key-options";

const CLI_RUNTIME_REFRESH_TIMEOUT_MS = 15_000;
const CLI_ACTIVATION_TIMEOUT_MS = 15_000;

interface UseCliRuntimeOptions {
  providers: Ref<Provider[]>;
  cliRuntime: Ref<CliRuntimeSnapshot>;
  agentRuntime: Ref<AgentRuntimeSnapshot>;
  refreshAgentRuntime: () => Promise<AgentRuntimeSnapshot>;
  activateAgentRuntime: (runtimeId: string) => Promise<void>;
  previewConfig: (
    providerId: string,
    cliKind: AgentCliKind,
    apiKeyLocalId: string,
  ) => Promise<AgentConfigurationEdit>;
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
  const cliConfigurationEditor = reactive(useAgentConfigurationEditor());
  let runtimeRefreshPending = false;
  let cliConfigRequestRevision = 0;
  let runtimeBridgeDisposed = false;
  let runtimeEventUnlisten: (() => void) | null = null;

  watch(cliConfigKeyPickerVisible, (visible) => {
    if (visible) return;
    cliConfigRequestRevision += 1;
    cliConfigKeyPickerProvider.value = null;
    cliConfigKeyPickerKind.value = null;
    cliConfigKeyPickerKeys.value = [];
  }, { flush: "sync" });

  watch(() => cliConfigurationEditor.visible, (visible) => {
    if (visible) return;
    cliConfigRequestRevision += 1;
    switchingCliConfig.value = null;
  }, { flush: "sync" });

  const cliInstancesProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === cliInstancesProviderId.value,
    ) ?? null,
  );

  const cliInstances = computed(() => {
    const providerLabels = new Map(
      options.providers.value.map((provider) => [provider.identity.id, providerDisplayLabel(provider)]),
    );
    return activeAgentRuntimeSessions(options.agentRuntime.value)
      .filter(
        (session) =>
          (!cliInstancesProviderId.value || session.provider?.providerId === cliInstancesProviderId.value) &&
          (!cliInstancesKind.value || session.agentKind === cliInstancesKind.value),
      )
      .map((session) => {
        if (!session.provider) return session;
        const providerName = providerLabels.get(session.provider.providerId);
        return providerName
          ? { ...session, provider: { ...session.provider, providerName } }
          : session;
      });
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
    if (runtimeRefreshPending) {
      return;
    }
    runtimeRefreshPending = true;
    if (!silent) {
      cliInstancesRefreshing.value = true;
    }
    try {
      await withTimeout(
        options.refreshAgentRuntime(),
        CLI_RUNTIME_REFRESH_TIMEOUT_MS,
        "读取 Agent runtime 状态超时",
      );
    } catch (error) {
      if (!silent) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      runtimeRefreshPending = false;
      if (!silent) {
        cliInstancesRefreshing.value = false;
      }
    }
  }

  function refreshOnWindowResume() {
    if (document.visibilityState === "hidden") return;
    void refreshCliRuntime(true);
  }

  onMounted(() => {
    runtimeBridgeDisposed = false;
    window.addEventListener("focus", refreshOnWindowResume);
    document.addEventListener("visibilitychange", refreshOnWindowResume);
    void (async () => {
      try {
        const unlisten = await listen<AgentRuntimeSnapshot>(
          "agent-runtime-updated",
          (event) => {
            if (!runtimeBridgeDisposed) {
              store.acceptAgentRuntimeSnapshot(event.payload);
            }
          },
        );
        if (runtimeBridgeDisposed) {
          unlisten();
        } else {
          runtimeEventUnlisten = unlisten;
        }
      } catch {
        // Event delivery is optional; the snapshot path remains usable.
      }
    })();
    void refreshCliRuntime(true);
  });

  onUnmounted(() => {
    runtimeBridgeDisposed = true;
    window.removeEventListener("focus", refreshOnWindowResume);
    document.removeEventListener("visibilitychange", refreshOnWindowResume);
    runtimeEventUnlisten?.();
    runtimeEventUnlisten = null;
    store.cancelAgentRuntimeRefresh();
  });

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
    cliConfigKeyPickerVisible.value = false;
    await previewProviderCliConfig(provider, cliKind, option);
  }

  async function previewProviderCliConfig(
    provider: Provider,
    cliKind: AgentCliKind,
    apiKey: ProviderApiKeyOption,
  ) {
    const providerId = provider.identity.id;
    const apiKeyLocalId = apiKey.localId.trim();
    cliConfigurationEditor.close();
    const requestRevision = ++cliConfigRequestRevision;
    switchingCliConfig.value = { providerId, cliKind };
    try {
      await cliConfigurationEditor.open(
        () => options.previewConfig(providerId, cliKind, apiKeyLocalId),
        { agentKind: cliKind, workspace: null, label: agentCliLabel(store.cliEnvironmentProbe, cliKind) + " 默认配置" },
        JSON.stringify(["provider", providerId, cliKind, apiKeyLocalId]),
      );
    } finally {
      if (requestRevision === cliConfigRequestRevision) switchingCliConfig.value = null;
    }
  }

  async function activateCliInstance(instance: AgentRuntimeSession) {
    if (!instance.actions.canActivateTerminal) return;
    activatingCliInstanceId.value = instance.runtimeId;
    try {
      await withTimeout(
        options.activateAgentRuntime(instance.runtimeId),
        CLI_ACTIVATION_TIMEOUT_MS,
        "激活 Agent runtime 终端超时",
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
    cliConfigurationEditor,
    openCliInstances,
    openAgentCliInstances,
    refreshCliRuntime,
    activateCliInstance,
    switchProviderCliConfig,
    selectCliConfigApiKey,
  };
}
