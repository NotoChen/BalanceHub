import { ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import { copyText } from "./useClipboard";

interface UseApiKeyManagerOptions {
  listKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  createKey: (providerId: string, name: string) => Promise<ProviderApiKeyOption[]>;
  deleteKey: (providerId: string, tokenId: string) => Promise<ProviderApiKeyOption[]>;
  getProvider: (providerId: string) => Provider | undefined;
}

export function useApiKeyManager(options: UseApiKeyManagerOptions) {
  const apiKeyManagerVisible = ref(false);
  const apiKeyManagerProvider = ref<Provider | null>(null);
  const apiKeyManagerLoading = ref(false);
  const apiKeyManagerKeys = ref<ProviderApiKeyOption[]>([]);
  const apiKeyCreateVisible = ref(false);
  const apiKeyCreateName = ref("");
  let requestRevision = 0;

  function currentRequest(providerId: string, revision: number) {
    return (
      revision === requestRevision &&
      apiKeyManagerVisible.value &&
      apiKeyManagerProvider.value?.identity.id === providerId
    );
  }

  function openApiKeyManager(provider: Provider) {
    requestRevision += 1;
    apiKeyManagerProvider.value = provider;
    apiKeyManagerKeys.value = [];
    apiKeyCreateVisible.value = false;
    apiKeyCreateName.value = "";
    apiKeyManagerVisible.value = true;
    void refreshApiKeyManager();
  }

  function openApiKeyCreateModal() {
    apiKeyCreateName.value = "";
    apiKeyCreateVisible.value = true;
  }

  async function refreshApiKeyManager() {
    if (!apiKeyManagerProvider.value) return;
    const providerId = apiKeyManagerProvider.value.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerLoading.value = true;
    try {
      const keys = await options.listKeys(providerId);
      if (!currentRequest(providerId, revision)) return;
      apiKeyManagerKeys.value = keys;
      apiKeyManagerProvider.value = options.getProvider(providerId) ?? apiKeyManagerProvider.value;
    } catch (error) {
      if (currentRequest(providerId, revision)) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (currentRequest(providerId, revision)) {
        apiKeyManagerLoading.value = false;
      }
    }
  }

  async function createManagedApiKey() {
    if (!apiKeyManagerProvider.value) return;
    const name = apiKeyCreateName.value.trim();
    if (!name) {
      Message.warning("请填写 API 密钥名称");
      return;
    }
    const providerId = apiKeyManagerProvider.value.identity.id;
    const revision = ++requestRevision;
    apiKeyManagerLoading.value = true;
    try {
      const keys = await options.createKey(providerId, name);
      if (!currentRequest(providerId, revision)) return;
      apiKeyManagerKeys.value = keys;
      apiKeyManagerProvider.value = options.getProvider(providerId) ?? apiKeyManagerProvider.value;
      apiKeyCreateName.value = "";
      apiKeyCreateVisible.value = false;
      Message.success("已创建 API 密钥");
    } catch (error) {
      if (currentRequest(providerId, revision)) {
        Message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (currentRequest(providerId, revision)) {
        apiKeyManagerLoading.value = false;
      }
    }
  }

  async function copyManagedApiKey(option: ProviderApiKeyOption) {
    if (!option.keyAvailable || !option.key.trim()) {
      Message.warning("该 API Key 未读取到完整值，无法复制");
      return;
    }
    try {
      await copyText(option.key);
      Message.success("已复制 API 密钥");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function deleteManagedApiKey(option: ProviderApiKeyOption) {
    if (!apiKeyManagerProvider.value || !option.tokenId) return;
    const providerId = apiKeyManagerProvider.value.identity.id;
    const managerRevision = requestRevision;
    Modal.confirm({
      title: "删除 API 密钥",
      content: `确定删除“${option.name || "API 密钥"}”吗？`,
      okText: "删除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: async () => {
        if (!currentRequest(providerId, managerRevision)) return;
        const revision = ++requestRevision;
        apiKeyManagerLoading.value = true;
        try {
          const keys = await options.deleteKey(providerId, option.tokenId);
          if (!currentRequest(providerId, revision)) return;
          apiKeyManagerKeys.value = keys;
          apiKeyManagerProvider.value = options.getProvider(providerId) ?? apiKeyManagerProvider.value;
          Message.success("已删除 API 密钥");
        } catch (error) {
          if (currentRequest(providerId, revision)) {
            Message.error(error instanceof Error ? error.message : String(error));
          }
        } finally {
          if (currentRequest(providerId, revision)) {
            apiKeyManagerLoading.value = false;
          }
        }
      },
    });
  }

  return {
    apiKeyManagerVisible,
    apiKeyManagerProvider,
    apiKeyManagerLoading,
    apiKeyManagerKeys,
    apiKeyCreateVisible,
    apiKeyCreateName,
    openApiKeyManager,
    openApiKeyCreateModal,
    refreshApiKeyManager,
    createManagedApiKey,
    copyManagedApiKey,
    deleteManagedApiKey,
  };
}
