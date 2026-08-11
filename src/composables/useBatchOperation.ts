import { ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { Channel } from "@tauri-apps/api/core";
import {
  checkInAllProviders,
  refreshAllProvidersWithProgress,
  type ProviderBatchOperation,
  type ProviderBatchProgressEvent,
  type ProviderBatchProgressItem,
} from "../api/batch-operation";
import type { Provider } from "../stores/providers";

interface UseBatchOperationOptions {
  providers: Ref<Provider[]>;
  replaceProviders: (providers: Provider[]) => void;
  setRefreshInProgress?: (value: boolean) => void;
  refreshCliRuntime?: () => Promise<unknown>;
  notifySystem?: (
    title: string,
    body: string,
    options?: { ignoreSwitch?: boolean; provider?: Provider },
  ) => Promise<boolean>;
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
      if (event.event === "providerFinished" && operation.value === "checkIn") {
        notifyCheckInResult(event.data.item);
      }
      return;
    }
    completed.value = true;
  }

  function notifyCheckInResult(item: ProviderBatchProgressItem) {
    if (!options.notifySystem || item.status === "skipped") return;
    const provider = options.providers.value.find(
      (candidate) => candidate.identity.id === item.providerId,
    );
    if (!provider) return;
    const title = item.status === "success" ? "BalanceHub 签到成功" : "BalanceHub 签到失败";
    void options.notifySystem(
      title,
      `**中转站**：${provider.identity.name}\n\n**结果**：${item.message}`,
      { provider },
    );
  }

  function markRefreshing(operationKind: ProviderBatchOperation) {
    const shouldMark = (provider: Provider) => {
      if (!provider.runtime.enabled) return false;
      if (operationKind === "refresh") return true;
      return provider.actions.checkIn && !provider.actions.checkedInToday;
    };
    options.replaceProviders(
      options.providers.value.map((provider) =>
        shouldMark(provider)
          ? {
              ...provider,
              runtime: { ...provider.runtime, status: "syncing", errorMessage: null },
            }
          : provider,
      ),
    );
  }

  function markCommandFailure(message: string, previousProviders: Provider[]) {
    options.replaceProviders(
      previousProviders.map((provider) => {
        const marked =
          provider.runtime.enabled &&
          (operation.value === "refresh" ||
            (operation.value === "checkIn" &&
              provider.actions.checkIn &&
              !provider.actions.checkedInToday));
        return marked
          ? {
              ...provider,
              runtime: { ...provider.runtime, status: "error", errorMessage: message },
            }
          : provider;
      }),
    );
  }

  async function run(operationKind: ProviderBatchOperation) {
    if (running.value) {
      visible.value = true;
      return;
    }

    const sequence = ++runSequence;
    operation.value = operationKind;
    running.value = true;
    visible.value = true;
    completed.value = false;
    error.value = "";
    items.value = [];
    startedAt.value = Date.now();
    finishedAt.value = null;
    const previousProviders = options.providers.value;
    markRefreshing(operationKind);
    if (operationKind === "refresh") {
      options.setRefreshInProgress?.(true);
    }

    try {
      const channel = new Channel<ProviderBatchProgressEvent>(handleEvent);
      const result =
        operationKind === "refresh"
          ? await refreshAllProvidersWithProgress(channel)
          : await checkInAllProviders(channel);
      if (sequence !== runSequence) return;
      options.replaceProviders(result.providers);
      if (operationKind === "refresh") {
        if (options.refreshCliRuntime) {
          await options.refreshCliRuntime().catch(() => {});
        }
      }
      completed.value = true;
    } catch (cause) {
      if (sequence !== runSequence) return;
      error.value = cause instanceof Error ? cause.message : String(cause);
      markCommandFailure(error.value, previousProviders);
      Message.error(
        operationKind === "refresh" ? `刷新失败：${error.value}` : `签到失败：${error.value}`,
      );
    } finally {
      if (sequence === runSequence) {
        running.value = false;
        finishedAt.value = Date.now();
        if (operationKind === "refresh") {
          options.setRefreshInProgress?.(false);
        }
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
    runRefresh: () => run("refresh"),
    runCheckIn: () => run("checkIn"),
  };
}
