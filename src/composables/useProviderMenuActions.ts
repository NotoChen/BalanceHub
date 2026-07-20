import { computed, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { openCcSwitchDeeplink, userHomeDir } from "../api/app";
import type { LivenessCliKind, Provider, TemporaryCliInstance } from "../stores/providers";
import { useProviderContextMenu } from "./useProviderContextMenu";
import { useProviderCopyActions } from "./useProviderCopyActions";
import {
  buildCcSwitchProviderDeeplink,
  canBuildCcSwitchDeeplink,
  ccSwitchTargetLabels,
  type CcSwitchAppTarget,
} from "../utils/ccswitch-deeplink";

interface UseProviderMenuActionsOptions {
  providers: { value: Provider[] };
  refreshByIds: (ids: string[]) => Promise<unknown>;
  testLiveness: (id: string) => Promise<{ record: { ok: boolean; responsePreview: string; message: string } }>;
  launchTemporaryCli: (
    id: string,
    cliKind: LivenessCliKind,
    workdir: string,
  ) => Promise<TemporaryCliInstance>;
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

export function useProviderMenuActions(options: UseProviderMenuActionsOptions) {
  const probingCapabilitiesProviderId = ref<string | null>(null);
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
      probingCapabilitiesProviderId.value = id;
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

  async function launchTemporaryCliFromMenu(provider: Provider, cliKind: LivenessCliKind) {
    closeProviderContextMenu();
    if (!provider.identity.baseUrl.trim() || !provider.auth.apiKey.trim()) {
      Message.warning("临时启动 CLI 需要中转站地址和 API Key");
      return;
    }

    let defaultPath: string | null = null;
    try {
      defaultPath = await userHomeDir();
    } catch {
      defaultPath = null;
    }

    const selected = await open({
      title: "选择 CLI 工作目录",
      directory: true,
      multiple: false,
      defaultPath: defaultPath ?? undefined,
    });
    if (!selected || Array.isArray(selected)) {
      return;
    }

    try {
      await options.launchTemporaryCli(provider.identity.id, cliKind, selected);
      Message.success(`已启动 ${cliKind === "codex" ? "Codex" : "Claude Code"}`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
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

  async function probeProviderCapabilitiesFromMenu(provider: Provider) {
    closeProviderContextMenu();
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

  async function copyInviteLinkFromMenu(provider: Provider) {
    closeProviderContextMenu();
    await copyInviteLink(provider);
  }

  function openApiKeyManagerFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openApiKeyManager(provider);
  }

  function openAvailableModelsFromMenu(provider: Provider) {
    closeProviderContextMenu();
    options.openAvailableModels(provider);
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

  async function addCcSwitchConfigFromMenu(provider: Provider, target: CcSwitchAppTarget) {
    closeProviderContextMenu();
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
    probingCapabilitiesProviderId,
    testingLivenessProviderId,
    livenessDetailsVisible,
    livenessDetailsProvider,
    closeProviderContextMenu,
    openProviderContextMenu,
    editProviderFromMenu,
    refreshProviderFromMenu,
    testLivenessFromMenu,
    launchTemporaryCliFromMenu,
    checkInProviderFromMenu,
    copyProviderUrlFromMenu,
    copyProviderSecretFromMenu,
    probeProviderCapabilitiesFromMenu,
    copyInviteLinkFromMenu,
    openApiKeyManagerFromMenu,
    openAvailableModelsFromMenu,
    openUsageFromMenu,
    openRequestLogsFromMenu,
    openPasswordChangeFromMenu,
    openLivenessDetailsFromMenu,
    openCheckInRecordsFromMenu,
    addCcSwitchConfigFromMenu,
    toggleProviderFromMenu,
    removeProviderFromMenu,
  };
}
