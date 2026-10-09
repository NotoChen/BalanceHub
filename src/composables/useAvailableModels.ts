import { computed, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import type { ProviderModelSyncResult, Provider } from "../stores/providers";
import { copyText } from "./useClipboard";
import { useLatestRequest } from "./useLatestRequest";

interface UseAvailableModelsOptions {
  providers: { value: Provider[] };
  syncModels: (providerId: string) => Promise<ProviderModelSyncResult>;
}

export function useAvailableModels(options: UseAvailableModelsOptions) {
  const availableModelsVisible = ref(false);
  const availableModelsProviderId = ref<string | null>(null);
  const request = useLatestRequest({ timeoutMessage: "获取模型列表超时，请重试" });

  const availableModelsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === availableModelsProviderId.value) ?? null,
  );

  watch([availableModelsVisible, () => availableModelsProvider.value?.identity.id,
    () => availableModelsProvider.value?.auth.credentialRevision],
    request.invalidate, { flush: "sync" });

  function openAvailableModels(provider: Provider) {
    availableModelsProviderId.value = provider.identity.id;
    availableModelsVisible.value = true;

    if (provider.actions.models.canSync && !provider.capabilities.availableModelsState.updatedAt) {
      void refreshAvailableModels();
    }
  }

  async function refreshAvailableModels() {
    const provider = availableModelsProvider.value;
    if (!availableModelsVisible.value || !provider || request.loading.value) {
      return;
    }
    if (!provider.actions.models.canSync) {
      request.error.value = provider.actions.models.unavailableReason || "当前认证信息无法获取模型列表";
      return;
    }

    await request.run(() => options.syncModels(provider.identity.id), (result) => {
      Message.success(result.message || `已获取 ${result.models.length} 个模型`);
    });
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

  async function copyAvailableModels(models: string[]) {
    const value = models.map((model) => model.trim()).filter(Boolean).join("\n");
    if (!value) {
      Message.warning("暂无可复制的模型");
      return;
    }
    try {
      await copyText(value);
      Message.success(`已复制 ${models.length} 个模型名称`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  return {
    availableModelsVisible,
    availableModelsProvider,
    availableModelsLoading: request.loading,
    availableModelsError: computed(() => request.error.value
      || (request.loading.value ? "" : availableModelsProvider.value?.capabilities.availableModelsState.error || "")),
    openAvailableModels,
    refreshAvailableModels,
    copyAvailableModel,
    copyAvailableModels,
  };
}
