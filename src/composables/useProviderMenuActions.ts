import { computed, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider } from "../stores/providers";
import { useProviderContextMenu } from "./useProviderContextMenu";
import { useProviderCopyActions } from "./useProviderCopyActions";

interface UseProviderMenuActionsOptions {
  providers: { value: Provider[] };
  refreshByIds: (ids: string[]) => Promise<unknown>;
  testLiveness: (id: string) => Promise<{ record: { ok: boolean; responsePreview: string; message: string } }>;
  syncCapabilities: (id: string) => Promise<{ provider: Provider; message: string }>;
  getInviteLink: (id: string) => Promise<string>;
  reload: () => Promise<unknown>;
  openEditProvider: (provider: Provider) => void;
  checkInProviderAction: (provider: Provider) => Promise<void>;
  openApiKeyManager: (provider: Provider) => void;
  openUsage: (provider: Provider) => void;
  openRequestLogs: (provider: Provider) => void;
  openPasswordChange: (provider: Provider) => void;
  openCheckInRecords: (provider: Provider) => void;
  toggleProvider: (provider: Provider, enabled: boolean) => Promise<void>;
  removeProvider: (provider: Provider) => Promise<void>;
}

export function useProviderMenuActions(options: UseProviderMenuActionsOptions) {
  const syncingCapabilitiesProviderId = ref<string | null>(null);
  const testingLivenessProviderId = ref<string | null>(null);
  const livenessDetailsVisible = ref(false);
  const livenessDetailsProviderId = ref<string | null>(null);
  const {
    providerContextMenu,
    closeProviderContextMenu,
    openProviderContextMenu,
  } = useProviderContextMenu();
  const {
    copyProviderUrl,
    copyProviderSecret,
    copyInviteLink,
  } = useProviderCopyActions({
    getInviteLink: options.getInviteLink,
    reload: options.reload,
    setBusyProviderId: (id) => {
      syncingCapabilitiesProviderId.value = id;
    },
  });

  const livenessDetailsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === livenessDetailsProviderId.value) ?? null,
  );

  function editProviderFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openEditProvider(provider);
  }

  function refreshProviderFromMenu(provider: Provider) {
    closeProviderContextMenu();
    if (!provider.runtime.enabled) {
      return;
    }
    void options.refreshByIds([provider.identity.id]);
  }

  async function testLivenessFromMenu(provider: Provider) {
    closeProviderContextMenu();
    if (!provider.runtime.enabled) {
      return;
    }
    if (!provider.auth.apiKey.trim()) {
      Message.warning("测活需要 API Key");
      return;
    }

    testingLivenessProviderId.value = provider.identity.id;
    try {
      const result = await options.testLiveness(provider.identity.id);
      if (result.record.ok) {
        Message.success(`测活成功：${result.record.responsePreview || result.record.message}`);
      } else {
        Message.error(`测活失败：${result.record.message}`);
      }
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      testingLivenessProviderId.value = null;
    }
  }

  function checkInProviderFromMenu(provider: Provider) {
    closeProviderContextMenu();
    if (!provider.runtime.enabled) {
      return;
    }
    void options.checkInProviderAction(provider);
  }

  async function copyProviderUrlFromMenu(provider: Provider) {
    closeProviderContextMenu();
    await copyProviderUrl(provider);
  }

  async function copyProviderSecretFromMenu(
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ) {
    closeProviderContextMenu();
    await copyProviderSecret(provider, field);
  }

  async function syncProviderCapabilitiesFromMenu(provider: Provider) {
    closeProviderContextMenu();
    if (!provider.runtime.enabled) {
      return;
    }

    syncingCapabilitiesProviderId.value = provider.identity.id;
    try {
      const result = await options.syncCapabilities(provider.identity.id);
      const errorMessage = result.provider.capabilities.errorMessage?.trim();
      if (errorMessage) {
        Message.warning(`站点能力已同步，部分能力不可用：${errorMessage}`);
      } else {
        Message.success(result.message || "站点能力已同步");
      }
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      syncingCapabilitiesProviderId.value = null;
    }
  }

  async function copyInviteLinkFromMenu(provider: Provider) {
    closeProviderContextMenu();
    await copyInviteLink(provider);
  }

  function openApiKeyManagerFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openApiKeyManager(provider);
  }

  function openUsageFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openUsage(provider);
  }

  function openRequestLogsFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openRequestLogs(provider);
  }

  function openPasswordChangeFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openPasswordChange(provider);
  }

  function openLivenessDetailsFromMenu(provider: Provider) {
    closeProviderContextMenu();
    livenessDetailsProviderId.value = provider.identity.id;
    livenessDetailsVisible.value = true;
  }

  function openCheckInRecordsFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openCheckInRecords(provider);
  }

  function toggleProviderFromMenu(provider: Provider) {
    closeProviderContextMenu();
    void options.toggleProvider(provider, !provider.runtime.enabled);
  }

  function removeProviderFromMenu(provider: Provider) {
    closeProviderContextMenu();
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
    providerContextMenu,
    syncingCapabilitiesProviderId,
    testingLivenessProviderId,
    livenessDetailsVisible,
    livenessDetailsProvider,
    closeProviderContextMenu,
    openProviderContextMenu,
    editProviderFromMenu,
    refreshProviderFromMenu,
    testLivenessFromMenu,
    checkInProviderFromMenu,
    copyProviderUrlFromMenu,
    copyProviderSecretFromMenu,
    syncProviderCapabilitiesFromMenu,
    copyInviteLinkFromMenu,
    openApiKeyManagerFromMenu,
    openUsageFromMenu,
    openRequestLogsFromMenu,
    openPasswordChangeFromMenu,
    openLivenessDetailsFromMenu,
    openCheckInRecordsFromMenu,
    toggleProviderFromMenu,
    removeProviderFromMenu,
  };
}
