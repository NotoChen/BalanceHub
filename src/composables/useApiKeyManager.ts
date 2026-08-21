import { computed, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import { providerApiKeyDisplayName } from "../utils/provider-display";
import { copyText } from "./useClipboard";

interface UseApiKeyManagerOptions {
  listLocalKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  syncRemoteKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  addLocalKey: (providerId: string, key: string, remark: string) => Promise<Provider>;
  createRemoteKey: (providerId: string, name: string) => Promise<ProviderApiKeyOption[]>;
  setRemark: (providerId: string, localId: string, remark: string) => Promise<Provider>;
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
  const apiKeyAddRemark = ref("");
  const apiKeyAddValue = ref("");
  const apiKeyRemarkVisible = ref(false);
  const apiKeyRemarkValue = ref("");
  const apiKeyRemarkTarget = ref<ProviderApiKeyOption | null>(null);
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
    apiKeyRemarkVisible.value = false;
    apiKeyCreateName.value = "";
    apiKeyAddRemark.value = "";
    apiKeyAddValue.value = "";
    apiKeyRemarkValue.value = "";
    apiKeyRemarkTarget.value = null;
    apiKeyManagerVisible.value = true;
    void refreshApiKeyManager();
  }

  function openApiKeyCreateModal() {
    apiKeyCreateName.value = "";
    apiKeyCreateVisible.value = true;
  }

  function openApiKeyAddModal() {
    apiKeyAddRemark.value = "";
    apiKeyAddValue.value = "";
    apiKeyAddVisible.value = true;
  }

  function openApiKeyRemarkModal(option: ProviderApiKeyOption) {
    apiKeyRemarkTarget.value = option;
    apiKeyRemarkValue.value = option.localName?.trim() || "";
    apiKeyRemarkVisible.value = true;
  }

  async function refreshApiKeyManager() {
    const provider = apiKeyManagerProvider.value;
    if (!provider) return;
    const providerId = provider.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerLoading.value = true;
    try {
      const localKeys = await options.listLocalKeys(providerId);
      if (!currentRequest(providerId, revision)) return;
      apiKeyManagerKeys.value = localKeys;
      if (!apiKeyRemoteManaged.value) return;
      try {
        const remoteKeys = await options.syncRemoteKeys(providerId);
        if (!currentRequest(providerId, revision)) return;
        apiKeyManagerKeys.value = remoteKeys;
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
      const keys = await options.createRemoteKey(providerId, name);
      if (!currentRequest(providerId, revision)) return;
      apiKeyManagerKeys.value = keys;
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
    const remark = apiKeyAddRemark.value.trim();
    if (!key) return Message.warning("请填写 API Key");
    try {
      await runProviderMutation(
        (providerId) => options.addLocalKey(providerId, key, remark),
        "已加入本地密钥库",
      );
      apiKeyAddVisible.value = false;
      apiKeyAddRemark.value = "";
      apiKeyAddValue.value = "";
    } catch {
      // Error is surfaced by runProviderMutation.
    }
  }

  async function saveManagedApiKeyRemark() {
    const target = apiKeyRemarkTarget.value;
    const remark = apiKeyRemarkValue.value.trim();
    if (!target) return;
    try {
      await runProviderMutation(
        (providerId) => options.setRemark(providerId, target.localId, remark),
        remark ? "已保存 API Key 本地备注" : "已清空 API Key 本地备注",
      );
      apiKeyRemarkVisible.value = false;
      apiKeyRemarkTarget.value = null;
      apiKeyRemarkValue.value = "";
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
        `已将“${providerApiKeyDisplayName(option)}”设为主 Key`,
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
        ? `确定从站点删除“${providerApiKeyDisplayName(option)}”吗？此操作会撤销站点上的令牌。`
        : `确定从本机密钥库移除“${providerApiKeyDisplayName(option)}”吗？不会影响站点上的其他令牌。`,
      okText: remote ? "删除" : "移除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: async () => {
        if (remote) {
          const providerId = provider.identity.id;
          const revision = ++requestRevision;
          apiKeyManagerLoading.value = true;
          try {
            const keys = await options.deleteRemoteKey(providerId, option.tokenId);
            if (!currentRequest(providerId, revision)) return;
            apiKeyManagerKeys.value = keys;
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
    apiKeyAddRemark,
    apiKeyAddValue,
    apiKeyRemarkVisible,
    apiKeyRemarkValue,
    apiKeyRemarkTarget,
    openApiKeyManager,
    openApiKeyCreateModal,
    openApiKeyAddModal,
    openApiKeyRemarkModal,
    refreshApiKeyManager,
    createManagedApiKey,
    addLocalApiKey,
    saveManagedApiKeyRemark,
    setPrimaryManagedApiKey,
    copyManagedApiKey,
    deleteManagedApiKey,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
