import { ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import type { Provider, ProviderApiKeyOption, ProviderInput } from "../stores/providers";
import { providerToInput } from "../utils/provider-input";
import { copyText } from "./useClipboard";

interface UseApiKeyManagerOptions {
  listKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  createKey: (providerId: string, name: string) => Promise<ProviderApiKeyOption[]>;
  deleteKey: (providerId: string, tokenId: string) => Promise<ProviderApiKeyOption[]>;
  saveProvider: (input: ProviderInput) => Promise<Provider[]>;
}

export function useApiKeyManager(options: UseApiKeyManagerOptions) {
  const apiKeyManagerVisible = ref(false);
  const apiKeyManagerProvider = ref<Provider | null>(null);
  const apiKeyManagerLoading = ref(false);
  const apiKeyManagerKeys = ref<ProviderApiKeyOption[]>([]);
  const apiKeyCreateVisible = ref(false);
  const apiKeyCreateName = ref("");

  function openApiKeyManager(provider: Provider) {
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
    apiKeyManagerLoading.value = true;
    try {
      apiKeyManagerKeys.value = await options.listKeys(apiKeyManagerProvider.value.identity.id);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      apiKeyManagerLoading.value = false;
    }
  }

  async function createManagedApiKey() {
    if (!apiKeyManagerProvider.value) return;
    const name = apiKeyCreateName.value.trim();
    if (!name) {
      Message.warning("请填写 API 密钥名称");
      return;
    }
    apiKeyManagerLoading.value = true;
    try {
      apiKeyManagerKeys.value = await options.createKey(apiKeyManagerProvider.value.identity.id, name);
      apiKeyCreateName.value = "";
      apiKeyCreateVisible.value = false;
      Message.success("已创建 API 密钥");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      apiKeyManagerLoading.value = false;
    }
  }

  async function copyManagedApiKey(option: ProviderApiKeyOption) {
    try {
      await copyText(option.key);
      Message.success("已复制 API 密钥");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function deleteManagedApiKey(option: ProviderApiKeyOption) {
    if (!apiKeyManagerProvider.value || !option.tokenId) return;
    Modal.confirm({
      title: "删除 API 密钥",
      content: `确定删除“${option.name || "API 密钥"}”吗？`,
      okText: "删除",
      cancelText: "取消",
      okButtonProps: { status: "danger" },
      onOk: async () => {
        if (!apiKeyManagerProvider.value) return;
        apiKeyManagerLoading.value = true;
        try {
          apiKeyManagerKeys.value = await options.deleteKey(
            apiKeyManagerProvider.value.identity.id,
            option.tokenId,
          );
          Message.success("已删除 API 密钥");
        } catch (error) {
          Message.error(error instanceof Error ? error.message : String(error));
        } finally {
          apiKeyManagerLoading.value = false;
        }
      },
    });
  }

  async function useManagedApiKey(option: ProviderApiKeyOption) {
    if (!apiKeyManagerProvider.value) return;
    const provider = apiKeyManagerProvider.value;
    const savedProviders = await options.saveProvider(
      providerToInput(provider, { auth: { ...provider.auth, apiKey: option.key } }),
    );
    apiKeyManagerProvider.value =
      savedProviders.find((item) => item.identity.id === provider.identity.id) ?? apiKeyManagerProvider.value;
    Message.success("已更新当前 API 密钥");
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
    useManagedApiKey,
  };
}
