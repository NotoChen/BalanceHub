import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { CodexModelSyncResult, Provider } from "../stores/providers";
import { copyText } from "./useClipboard";

interface UseAvailableModelsOptions {
  providers: { value: Provider[] };
  syncModels: (providerId: string) => Promise<CodexModelSyncResult>;
}

export function useAvailableModels(options: UseAvailableModelsOptions) {
  const availableModelsVisible = ref(false);
  const availableModelsProviderId = ref<string | null>(null);
  const availableModelsLoading = ref(false);

  const availableModelsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === availableModelsProviderId.value) ?? null,
  );

  function openAvailableModels(provider: Provider) {
    availableModelsProviderId.value = provider.identity.id;
    availableModelsVisible.value = true;

    if (provider.auth.apiKey.trim() && (provider.capabilities.availableModels || []).length === 0) {
      void refreshAvailableModels();
    }
  }

  async function refreshAvailableModels() {
    const provider = availableModelsProvider.value;
    if (!provider) {
      return;
    }
    if (!provider.auth.apiKey.trim()) {
      Message.warning("获取模型列表需要 API Key");
      return;
    }

    availableModelsLoading.value = true;
    try {
      const result = await options.syncModels(provider.identity.id);
      Message.success(result.message || `已获取 ${result.models.length} 个模型`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      availableModelsLoading.value = false;
    }
  }

  async function copyAvailableModel(model: string) {
    const value = model.trim();
    if (!value) {
      return;
    }
    try {
      await copyText(value);
      Message.success("已复制模型名称");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function copyAllAvailableModels() {
    const models = availableModelsProvider.value?.capabilities.availableModels ?? [];
    const value = models.map((model) => model.trim()).filter(Boolean).join("\n");
    if (!value) {
      Message.warning("暂无可复制的模型");
      return;
    }
    try {
      await copyText(value);
      Message.success("已复制全部模型");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  return {
    availableModelsVisible,
    availableModelsProvider,
    availableModelsLoading,
    openAvailableModels,
    refreshAvailableModels,
    copyAvailableModel,
    copyAllAvailableModels,
  };
}
