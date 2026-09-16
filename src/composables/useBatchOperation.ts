import { ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { Channel } from "@tauri-apps/api/core";
import {
  refreshAllProvidersWithProgress,
  type ProviderBatchOperation,
  type ProviderBatchProgressEvent,
  type ProviderBatchProgressItem,
} from "../api/batch-operation";
import type { Provider } from "../stores/providers";

interface UseBatchOperationOptions {
  providers: Ref<Provider[]>;
  replaceProviders: (providers: Provider[]) => void;
  upsertProviders: (providers: Provider[]) => void;
  setRefreshInProgress?: (value: boolean) => void;
  refreshCliRuntime?: () => Promise<unknown>;
}

export function useBatchOperation(options: UseBatchOperationOptions) {
  const operation = ref<ProviderBatchOperation | null>(null);
  const running = ref(false);
  const visible = ref(false);
  const items = ref<ProviderBatchProgressItem[]>([]);
  const error = ref("");
  const startedAt = ref<number | null>(null);
  const finishedAt = ref<number | null>(null);
  const completed = ref(false);
  let runSequence = 0;

  function updateItem(next: ProviderBatchProgressItem) {
    const index = items.value.findIndex((item) => item.providerId === next.providerId);
    if (index < 0) {
      items.value = [...items.value, next];
      return;
    }
    const nextItems = [...items.value];
    nextItems[index] = next;
    items.value = nextItems;
  }

  function handleEvent(event: ProviderBatchProgressEvent) {
    if (event.event === "started") {
      if (event.data.operation !== operation.value) return;
      items.value = event.data.items;
      return;
    }
    if (event.data.operation !== operation.value) return;
    if (event.event === "providerStarted" || event.event === "providerFinished") {
      updateItem(event.data.item);
      return;
    }
    completed.value = true;
  }

  function markRefreshing() {
    options.replaceProviders(
      options.providers.value.map((provider) =>
        provider.runtime.enabled
          ? {
              ...provider,
              runtime: { ...provider.runtime, status: "syncing", errorMessage: null },
            }
          : provider,
      ),
    );
  }

  function markCommandFailure(message: string) {
    options.replaceProviders(
      options.providers.value.map((provider) => {
        return provider.runtime.enabled
          ? {
              ...provider,
              runtime: { ...provider.runtime, status: "error", errorMessage: message },
            }
          : provider;
      }),
    );
  }

  async function runRefresh() {
    if (running.value) {
      visible.value = true;
      return;
    }

    const sequence = ++runSequence;
    operation.value = "refresh";
    running.value = true;
    visible.value = true;
    completed.value = false;
    error.value = "";
    items.value = [];
    startedAt.value = Date.now();
    finishedAt.value = null;
    markRefreshing();
    options.setRefreshInProgress?.(true);

    try {
      const channel = new Channel<ProviderBatchProgressEvent>(handleEvent);
      const result = await refreshAllProvidersWithProgress(channel);
      if (sequence !== runSequence) return;
      options.upsertProviders(result.updatedProviders);
      await options.refreshCliRuntime?.().catch(() => {});
      completed.value = true;
    } catch (cause) {
      if (sequence !== runSequence) return;
      error.value = cause instanceof Error ? cause.message : String(cause);
      markCommandFailure(error.value);
      Message.error(`刷新失败：${error.value}`);
    } finally {
      if (sequence === runSequence) {
        running.value = false;
        finishedAt.value = Date.now();
        options.setRefreshInProgress?.(false);
      }
    }
  }

  return {
    operation,
    running,
    visible,
    items,
    error,
    startedAt,
    finishedAt,
    completed,
    runRefresh,
  };
}
