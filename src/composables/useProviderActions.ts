import { computed, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import { openCcSwitchDeeplink } from "../api/app";
import type { LivenessCliKind, Provider } from "../stores/providers";
import { supportsApiKeyManagement } from "../utils/provider-display";
import { useProviderCopyActions } from "./useProviderCopyActions";
import {
  buildCcSwitchProviderDeeplink,
  canBuildCcSwitchDeeplink,
  ccSwitchTargetLabels,
  type CcSwitchAppTarget,
} from "../utils/ccswitch-deeplink";

interface UseProviderActionsOptions {
  providers: { value: Provider[] };
  refreshByIds: (ids: string[]) => Promise<unknown>;
  openWorkspacePicker: (provider: Provider, cliKind?: LivenessCliKind) => void;
  probeCapabilities: (id: string) => Promise<{ provider: Provider; message: string }>;
  getInviteLink: (id: string) => Promise<string>;
  reload: () => Promise<unknown>;
  openEditProvider: (provider: Provider) => void;
  checkInProviderAction: (provider: Provider) => Promise<void>;
  openApiKeyManager: (provider: Provider) => void;
  openAvailableModels: (provider: Provider) => void;
  openUsage: (provider: Provider) => void;
  openRequestLogs: (provider: Provider) => void;
  openPasswordChange: (provider: Provider) => void;
  openCheckInRecords: (provider: Provider) => void;
  toggleProvider: (provider: Provider, enabled: boolean) => Promise<void>;
  removeProvider: (provider: Provider) => Promise<void>;
}

export function useProviderActions(options: UseProviderActionsOptions) {
  const probingCapabilitiesProviderId = ref<string | null>(null);
  const livenessDetailsVisible = ref(false);
  const livenessDetailsProviderId = ref<string | null>(null);
  const {
    copyProviderUrl,
    copyProviderSecret,
    copyInviteLink: copyInviteLinkValue,
  } = useProviderCopyActions({
    getInviteLink: options.getInviteLink,
    reload: options.reload,
    setBusyProviderId: (id) => {
      probingCapabilitiesProviderId.value = id;
    },
  });

  const livenessDetailsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === livenessDetailsProviderId.value) ?? null,
  );

  function editProvider(provider: Provider) {
    options.openEditProvider(provider);
  }

  function refreshProvider(provider: Provider) {
    if (!provider.runtime.enabled) {
      return;
    }
    void options.refreshByIds([provider.identity.id]);
  }

  function launchTemporaryCli(provider: Provider, cliKind?: LivenessCliKind) {
    if (!provider.identity.baseUrl.trim()) {
      Message.warning("临时启动 CLI 需要中转站地址");
      return;
    }
    if (!provider.auth.apiKey.trim() && !supportsApiKeyManagement(provider)) {
      Message.warning("临时启动 CLI 需要可用的 API Key");
      return;
    }
    options.openWorkspacePicker(provider, cliKind);
  }

  function checkInProvider(provider: Provider) {
    if (!provider.runtime.enabled) {
      return;
    }
    void options.checkInProviderAction(provider);
  }

  async function copyProviderUrlAction(provider: Provider) {
    await copyProviderUrl(provider);
  }

  async function copyProviderSecretAction(
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ) {
    await copyProviderSecret(provider, field);
  }

  async function probeProviderCapabilities(provider: Provider) {
    if (!provider.runtime.enabled) {
      return;
    }

    probingCapabilitiesProviderId.value = provider.identity.id;
    try {
      const result = await options.probeCapabilities(provider.identity.id);
      const errorMessage = result.provider.capabilities.errorMessage?.trim();
      if (errorMessage) {
        Message.warning(`站点能力已探测，部分能力不可用：${errorMessage}`);
      } else {
        Message.success(result.message || "站点能力已探测");
      }
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      probingCapabilitiesProviderId.value = null;
    }
  }

  async function copyInviteLinkAction(provider: Provider) {
    await copyInviteLinkValue(provider);
  }

  function openApiKeyManager(provider: Provider) {
    options.openApiKeyManager(provider);
  }

  function openAvailableModels(provider: Provider) {
    options.openAvailableModels(provider);
  }

  function openUsage(provider: Provider) {
    options.openUsage(provider);
  }

  function openRequestLogs(provider: Provider) {
    options.openRequestLogs(provider);
  }

  function openPasswordChange(provider: Provider) {
    options.openPasswordChange(provider);
  }

  function openLivenessDetails(provider: Provider) {
    livenessDetailsProviderId.value = provider.identity.id;
    livenessDetailsVisible.value = true;
  }

  function openCheckInRecords(provider: Provider) {
    options.openCheckInRecords(provider);
  }

  async function addCcSwitchConfig(provider: Provider, target: CcSwitchAppTarget) {
    if (!canBuildCcSwitchDeeplink(provider)) {
      Message.warning("添加到 CC Switch 需要中转站地址和 API Key");
      return;
    }

    const deeplink = buildCcSwitchProviderDeeplink(provider, target);
    try {
      await openCcSwitchDeeplink(deeplink);
      Message.success(`已请求打开 CC Switch：${ccSwitchTargetLabels[target]}`);
    } catch (error) {
      try {
        await navigator.clipboard.writeText(deeplink);
      } catch {
        // Ignore clipboard failures; the original open error is more useful.
      }
      Message.error(`无法打开 CC Switch，已尝试复制深链：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  function toggleProviderAction(provider: Provider) {
    void options.toggleProvider(provider, !provider.runtime.enabled);
  }

  function removeProviderAction(provider: Provider) {
    Modal.confirm({
      title: "删除中转站",
      content: `确定删除“${provider.identity.name}”吗？`,
      okText: "删除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: () => options.removeProvider(provider),
    });
  }

  return {
    probingCapabilitiesProviderId,
    livenessDetailsVisible,
    livenessDetailsProvider,
    editProvider,
    refreshProvider,
    launchTemporaryCli,
    checkInProvider,
    copyProviderUrlAction,
    copyProviderSecretAction,
    probeProviderCapabilities,
    copyInviteLinkAction,
    openApiKeyManager,
    openAvailableModels,
    openUsage,
    openRequestLogs,
    openPasswordChange,
    openLivenessDetails,
    openCheckInRecords,
    addCcSwitchConfig,
    toggleProviderAction,
    removeProviderAction,
  };
}
