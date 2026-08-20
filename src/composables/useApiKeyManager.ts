import { computed, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import { copyText } from "./useClipboard";

interface UseApiKeyManagerOptions {
  listLocalKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  syncRemoteKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  addLocalKey: (providerId: string, key: string, name: string) => Promise<Provider>;
  createRemoteKey: (providerId: string, name: string) => Promise<ProviderApiKeyOption[]>;
  renameKey: (providerId: string, localId: string, name: string) => Promise<Provider>;
  setPrimaryKey: (providerId: string, localId: string) => Promise<Provider>;
  removeLocalKey: (providerId: string, localId: string) => Promise<Provider>;
  deleteRemoteKey: (providerId: string, tokenId: string) => Promise<ProviderApiKeyOption[]>;
  getProvider: (providerId: string) => Provider | undefined;
}

export function useApiKeyManager(options: UseApiKeyManagerOptions) {
  const apiKeyManagerVisible = ref(false);
  const apiKeyManagerProvider = ref<Provider | null>(null);
  const apiKeyManagerLoading = ref(false);
  const apiKeyManagerKeys = ref<ProviderApiKeyOption[]>([]);
  const apiKeyCreateVisible = ref(false);
  const apiKeyCreateName = ref("");
  const apiKeyAddVisible = ref(false);
  const apiKeyAddName = ref("");
  const apiKeyAddValue = ref("");
  const apiKeyRenameVisible = ref(false);
  const apiKeyRenameName = ref("");
  const apiKeyRenameTarget = ref<ProviderApiKeyOption | null>(null);
  let requestRevision = 0;

  const apiKeyRemoteManaged = computed(() =>
    Boolean(apiKeyManagerProvider.value?.actions.apiKeyManagement),
  );

  function currentRequest(providerId: string, revision: number) {
    return revision === requestRevision
      && apiKeyManagerVisible.value
      && apiKeyManagerProvider.value?.identity.id === providerId;
  }

  function applyProvider(provider: Provider | null | undefined) {
    if (!provider) return;
    apiKeyManagerProvider.value = provider;
    apiKeyManagerKeys.value = [...(provider.auth.apiKeyOptions || [])];
  }

  function currentProvider(providerId: string) {
    return options.getProvider(providerId) ?? apiKeyManagerProvider.value;
  }

  function openApiKeyManager(provider: Provider) {
    requestRevision += 1;
    applyProvider(provider);
    apiKeyCreateVisible.value = false;
    apiKeyAddVisible.value = false;
    apiKeyRenameVisible.value = false;
    apiKeyCreateName.value = "";
    apiKeyAddName.value = "";
    apiKeyAddValue.value = "";
    apiKeyRenameName.value = "";
    apiKeyRenameTarget.value = null;
    apiKeyManagerVisible.value = true;
    void refreshApiKeyManager();
  }

  function openApiKeyCreateModal() {
    apiKeyCreateName.value = "";
    apiKeyCreateVisible.value = true;
  }

  function openApiKeyAddModal() {
    apiKeyAddName.value = "";
    apiKeyAddValue.value = "";
    apiKeyAddVisible.value = true;
  }

  function openApiKeyRenameModal(option: ProviderApiKeyOption) {
    apiKeyRenameTarget.value = option;
    apiKeyRenameName.value = option.localName || option.name || "";
    apiKeyRenameVisible.value = true;
  }

  async function refreshApiKeyManager() {
    const provider = apiKeyManagerProvider.value;
    if (!provider) return;
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerLoading.value = true;
    try {
      apiKeyManagerKeys.value = await options.listLocalKeys(providerId);
      if (!currentRequest(providerId, revision)) return;
      if (!apiKeyRemoteManaged.value) return;
      try {
        apiKeyManagerKeys.value = await options.syncRemoteKeys(providerId);
        if (!currentRequest(providerId, revision)) return;
        applyProvider(currentProvider(providerId));
      } catch (error) {
        if (currentRequest(providerId, revision)) {
          Message.warning(`站点密钥同步失败，本地密钥仍可使用：${errorMessage(error)}`);
        }
      }
    } catch (error) {
      if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerLoading.value = false;
    }
  }

  async function runProviderMutation(
    action: (providerId: string) => Promise<Provider>,
    success: string,
  ) {
    const provider = apiKeyManagerProvider.value;
    if (!provider) return;
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerLoading.value = true;
    try {
      const updated = await action(providerId);
      if (!currentRequest(providerId, revision)) return;
      applyProvider(updated);
      Message.success(success);
    } catch (error) {
      if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
      throw error;
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerLoading.value = false;
    }
  }

  async function createManagedApiKey() {
    const provider = apiKeyManagerProvider.value;
    const name = apiKeyCreateName.value.trim();
    if (!provider) return;
    if (!name) return Message.warning("请填写 API 密钥名称");
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerLoading.value = true;
    try {
      apiKeyManagerKeys.value = await options.createRemoteKey(providerId, name);
      if (!currentRequest(providerId, revision)) return;
      applyProvider(currentProvider(providerId));
      apiKeyCreateVisible.value = false;
      apiKeyCreateName.value = "";
      Message.success("已创建站点 API Key");
    } catch (error) {
      if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
    } finally {
      if (currentRequest(providerId, revision)) apiKeyManagerLoading.value = false;
    }
  }

  async function addLocalApiKey() {
    const key = apiKeyAddValue.value.trim();
    const name = apiKeyAddName.value.trim() || "本地 API Key";
    if (!key) return Message.warning("请填写 API Key");
    try {
      await runProviderMutation(
        (providerId) => options.addLocalKey(providerId, key, name),
        "已加入本地密钥库",
      );
      apiKeyAddVisible.value = false;
      apiKeyAddName.value = "";
      apiKeyAddValue.value = "";
    } catch {
      // Error is surfaced by runProviderMutation.
    }
  }

  async function renameManagedApiKey() {
    const target = apiKeyRenameTarget.value;
    const name = apiKeyRenameName.value.trim();
    if (!target) return;
    if (!name) return Message.warning("API Key 名称不能为空");
    try {
      await runProviderMutation(
        (providerId) => options.renameKey(providerId, target.localId, name),
        "已重命名 API Key",
      );
      apiKeyRenameVisible.value = false;
      apiKeyRenameTarget.value = null;
    } catch {
      // Error is surfaced by runProviderMutation.
    }
  }

  async function setPrimaryManagedApiKey(option: ProviderApiKeyOption) {
    if (!option.keyAvailable || !option.key.trim()) {
      return Message.warning("该 API Key 未读取到完整值，无法设为主 Key");
    }
    try {
      await runProviderMutation(
        (providerId) => options.setPrimaryKey(providerId, option.localId),
        `已将“${displayName(option)}”设为主 Key`,
      );
    } catch {
      // Error is surfaced by runProviderMutation.
    }
  }

  async function copyManagedApiKey(option: ProviderApiKeyOption) {
    if (!option.keyAvailable || !option.key.trim()) {
      return Message.warning("该 API Key 未读取到完整值，无法复制");
    }
    try {
      await copyText(option.key);
      Message.success("已复制 API Key");
    } catch (error) {
      Message.error(errorMessage(error));
    }
  }

  function deleteManagedApiKey(option: ProviderApiKeyOption) {
    const provider = apiKeyManagerProvider.value;
    if (!provider) return;
    const remote = Boolean(option.tokenId.trim());
    Modal.confirm({
      title: remote ? "删除站点 API Key" : "移除本地 API Key",
      content: remote
        ? `确定从站点删除“${displayName(option)}”吗？此操作会撤销站点上的令牌。`
        : `确定从本机密钥库移除“${displayName(option)}”吗？不会影响站点上的其他令牌。`,
      okText: remote ? "删除" : "移除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: async () => {
        if (remote) {
          const providerId = provider.identity.id;
          const revision = ++requestRevision;
          apiKeyManagerLoading.value = true;
          try {
            apiKeyManagerKeys.value = await options.deleteRemoteKey(providerId, option.tokenId);
            if (!currentRequest(providerId, revision)) return;
            applyProvider(currentProvider(providerId));
            Message.success("已删除站点 API Key");
          } catch (error) {
            if (currentRequest(providerId, revision)) Message.error(errorMessage(error));
          } finally {
            if (currentRequest(providerId, revision)) apiKeyManagerLoading.value = false;
          }
          return;
        }
        try {
          await runProviderMutation(
            (providerId) => options.removeLocalKey(providerId, option.localId),
            "已移除本地 API Key",
          );
        } catch {
          // Error is surfaced by runProviderMutation.
        }
      },
    });
  }

  return {
    apiKeyManagerVisible,
    apiKeyManagerProvider,
    apiKeyManagerLoading,
    apiKeyManagerKeys,
    apiKeyRemoteManaged,
    apiKeyCreateVisible,
    apiKeyCreateName,
    apiKeyAddVisible,
    apiKeyAddName,
    apiKeyAddValue,
    apiKeyRenameVisible,
    apiKeyRenameName,
    apiKeyRenameTarget,
    openApiKeyManager,
    openApiKeyCreateModal,
    openApiKeyAddModal,
    openApiKeyRenameModal,
    refreshApiKeyManager,
    createManagedApiKey,
    addLocalApiKey,
    renameManagedApiKey,
    setPrimaryManagedApiKey,
    copyManagedApiKey,
    deleteManagedApiKey,
  };
}

function displayName(option: ProviderApiKeyOption) {
  return option.localName || option.name || "API Key";
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
