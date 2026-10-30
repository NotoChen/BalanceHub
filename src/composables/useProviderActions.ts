import { computed, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { AgentCliKind, Provider } from "../stores/providers";
import { refreshAllCpa, startOAuthLogin } from "../api/oauth-accounts";
import { useOAuthAccountStore } from "../stores/oauth-accounts";
import { hasUsableProviderApiKey } from "../utils/provider-api-key-options";
import { useProviderCopyActions } from "./useProviderCopyActions";
import { useProviderAppImport } from "./useProviderAppImport";
import { providerDisplayLabel } from "../utils/provider-display";

interface UseProviderActionsOptions {
  providers: { value: Provider[] };
  refreshByIds: (ids: string[]) => Promise<unknown>;
  openWorkspacePicker: (provider: Provider, cliKind?: AgentCliKind) => void;
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
  const { addProviderToApp } = useProviderAppImport();
  const oauthAccounts = useOAuthAccountStore();
  const probingCapabilitiesProviderId = ref<string | null>(null);
  const capabilityProbeVisible = ref(false);
  const capabilityProbeProviderId = ref<string | null>(null);
  const capabilityProbeRunning = ref(false);
  const capabilityProbeError = ref("");
  const capabilityProbeResultMessage = ref("");
  const capabilityProbeStartedAt = ref<number | null>(null);
  const capabilityProbeFinishedAt = ref<number | null>(null);
  const inviteCopyingProviderId = ref<string | null>(null);
  const livenessDetailsVisible = ref(false);
  const livenessDetailsProviderId = ref<string | null>(null);
  /// One card-level authorization action runs at a time; the id doubles as the
  /// guard so a second click cannot start a parallel export or hand-off.
  const {
    copyProviderUrl,
    copyProviderSecret,
    copyInviteLink: copyInviteLinkValue,
  } = useProviderCopyActions({
    getInviteLink: options.getInviteLink,
    reload: options.reload,
    setBusyProviderId: (id) => {
      inviteCopyingProviderId.value = id;
    },
  });

  const livenessDetailsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === livenessDetailsProviderId.value) ?? null,
  );
  const capabilityProbeProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === capabilityProbeProviderId.value) ?? null,
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

  function launchTemporaryCli(provider: Provider, cliKind?: AgentCliKind) {
    // Rust decides whether the stored credential can start a CLI; the relay
    // address and API key checks only apply to relay cards.
    if (!provider.actions.temporaryCli) {
      Message.warning("当前账号缺少可用的授权，请先刷新或重新授权");
      return;
    }
    if (provider.identity.protocol !== "oauth") {
      if (!provider.identity.baseUrl.trim()) {
        Message.warning("临时启动 CLI 需要中转站地址");
        return;
      }
      if (!hasUsableProviderApiKey(provider.auth.apiKey, provider.auth.apiKeyOptions)) {
        Message.warning("临时启动 CLI 需要可用的 API Key");
        return;
      }
    }
    options.openWorkspacePicker(provider, cliKind);
  }

  function checkInProvider(provider: Provider) {
    if (!provider.runtime.enabled) {
      return;
    }
    void options.checkInProviderAction(provider);
  }

  /// The gateway refreshes every authorization in turn and invalidates the old
  /// tokens, so this stays behind the same confirmation the panel uses.
  function renewAllCpaAccounts(provider: Provider) {
    if (!provider.actions.cpaGateway || !provider.runtime.enabled) return;
    const revision = provider.auth.credentialRevision || 0;
    Modal.confirm({
      title: "请求 CPA 更新全部授权",
      content:
        "CPA 会依次为所有授权换取新令牌，旧令牌立即失效。仅当 CPA 是这些账号唯一的续期方时执行。",
      okText: "全部续期",
      cancelText: "取消",
      okButtonProps: { status: "warning" },
      onOk: async () => {
        await refreshAllCpa(provider.identity.id, revision);
        Message.success("已向 CPA 请求全部续期，稍后刷新核对");
        await options.reload();
      },
    });
  }

  /// A failed or unconfirmed renewal is only resolved by authorizing again, so
  /// the card can restart the same login flow the account panel uses and hand
  /// the task to the task center instead of keeping the card busy.
  async function reauthorizeOAuthCard(provider: Provider) {
    const account = provider.auth.oauth;
    if (!account) return;
    try {
      const task = await startOAuthLogin(account.vendor, provider.identity.id);
      oauthAccounts.accept(task);
      // 授权进度本身就是这次操作的结果，直接在被授权的同一个面板里看，
      // 不再让用户自己去任务中心找。
      oauthAccounts.openTask(task.id);
    } catch (cause) {
      Message.error(String(cause));
    }
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

    if (capabilityProbeRunning.value) {
      const activeProvider = capabilityProbeProvider.value;
      Message.info(
        activeProvider
          ? `正在探测“${providerDisplayLabel(activeProvider)}”，请等待当前任务完成`
          : "站点能力探测正在运行，请等待当前任务完成",
      );
      return;
    }

    capabilityProbeProviderId.value = provider.identity.id;
    capabilityProbeVisible.value = true;

    probingCapabilitiesProviderId.value = provider.identity.id;
    capabilityProbeRunning.value = true;
    capabilityProbeError.value = "";
    capabilityProbeResultMessage.value = "";
    capabilityProbeStartedAt.value = Date.now();
    capabilityProbeFinishedAt.value = null;
    try {
      const result = await options.probeCapabilities(provider.identity.id);
      if (capabilityProbeProviderId.value !== provider.identity.id) {
        return;
      }
      capabilityProbeResultMessage.value = result.message || "站点能力已探测";
      const errorMessage = result.provider.capabilities.errorMessage?.trim();
      if (errorMessage) {
        Message.warning(`站点能力已探测，部分能力不可用：${errorMessage}`);
      } else {
        Message.success(result.message || "站点能力已探测");
      }
    } catch (error) {
      if (capabilityProbeProviderId.value === provider.identity.id) {
        capabilityProbeError.value = error instanceof Error ? error.message : String(error);
        Message.error(capabilityProbeError.value);
      }
    } finally {
      if (capabilityProbeProviderId.value === provider.identity.id) {
        capabilityProbeRunning.value = false;
        capabilityProbeFinishedAt.value = Date.now();
        probingCapabilitiesProviderId.value = null;
      }
    }
  }

  function retryCapabilityProbe() {
    if (capabilityProbeProvider.value && !capabilityProbeRunning.value) {
      void probeProviderCapabilities(capabilityProbeProvider.value);
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

  function toggleProviderAction(provider: Provider) {
    void options.toggleProvider(provider, !provider.runtime.enabled);
  }

  function removeProviderAction(provider: Provider) {
    Modal.confirm({
      title: "删除中转站",
      content: `确定删除“${providerDisplayLabel(provider)}”吗？`,
      okText: "删除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: () => options.removeProvider(provider),
    });
  }

  return {
    probingCapabilitiesProviderId,
    capabilityProbeVisible,
    capabilityProbeProvider,
    capabilityProbeRunning,
    capabilityProbeError,
    capabilityProbeResultMessage,
    capabilityProbeStartedAt,
    capabilityProbeFinishedAt,
    retryCapabilityProbe,
    livenessDetailsVisible,
    livenessDetailsProvider,
    editProvider,
    refreshProvider,
    launchTemporaryCli,
    checkInProvider,
    renewAllCpaAccounts,
    reauthorizeOAuthCard,
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
    addProviderToApp,
    toggleProviderAction,
    removeProviderAction,
  };
}
