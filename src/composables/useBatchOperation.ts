import { onScopeDispose, ref, type Ref } from "vue";
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
  let disposed = false;
  let cancelWait: (() => void) | undefined;
  const refreshing = new Map<
    string,
    {
      revision: number;
      status: Provider["runtime"]["status"];
      errorMessage: Provider["runtime"]["errorMessage"];
    }
  >();

  function updateItem(next: ProviderBatchProgressItem) {
    const index = items.value.findIndex(
      (item) => item.providerId === next.providerId,
    );
    if (index < 0) {
      items.value = [...items.value, next];
      return;
    }
    const nextItems = [...items.value];
    nextItems[index] = next;
    items.value = nextItems;
  }

  function markProviderRefreshing(id: string) {
    const provider = options.providers.value.find(
      (item) => item.identity.id === id,
    );
    if (!provider?.runtime.enabled || provider.runtime.status === "syncing")
      return;
    refreshing.set(id, {
      revision: provider.revision,
      status: provider.runtime.status,
      errorMessage: provider.runtime.errorMessage,
    });
    options.replaceProviders(
      options.providers.value.map((item) =>
        item.identity.id === id
          ? {
              ...item,
              runtime: {
                ...item.runtime,
                status: "syncing",
                errorMessage: null,
              },
            }
          : item,
      ),
    );
  }

  function releaseProvider(id: string, failure?: string) {
    const previous = refreshing.get(id);
    refreshing.delete(id);
    if (!previous) return;
    options.replaceProviders(
      options.providers.value.map((provider) => {
        // A save, deletion or newer response takes ownership of the card. Only
        // undo the transient state written by this particular refresh attempt.
        if (
          provider.identity.id !== id ||
          provider.revision !== previous.revision ||
          provider.runtime.status !== "syncing"
        )
          return provider;
        return {
          ...provider,
          runtime: {
            ...provider.runtime,
            status: failure ? "error" : previous.status,
            errorMessage: failure ?? previous.errorMessage,
          },
        };
      }),
    );
  }

  function releaseRefreshing(failure?: string) {
    for (const id of refreshing.keys()) releaseProvider(id, failure);
  }

  function handleEvent(event: ProviderBatchProgressEvent) {
    if (event.data.operation !== operation.value) return;
    if (event.event === "started") {
      items.value = event.data.items;
      return;
    }
    if (event.event === "providerStarted") {
      const current = items.value.find(
        (item) => item.providerId === event.data.item.providerId,
      );
      if (current && ["success", "failed", "skipped"].includes(current.status))
        return;
      updateItem(event.data.item);
      markProviderRefreshing(event.data.item.providerId);
      return;
    }
    if (event.event === "providerFinished") {
      if (event.data.provider) options.upsertProviders([event.data.provider]);
      releaseProvider(
        event.data.item.providerId,
        event.data.item.status === "failed" && !event.data.provider
          ? event.data.item.message
          : undefined,
      );
      updateItem(event.data.item);
      return;
    }
    completed.value = true;
  }

  function markCommandFailure(message: string) {
    releaseRefreshing(message);
    items.value = items.value.map((item) =>
      ["pending", "running"].includes(item.status)
        ? { ...item, status: "failed", message: `刷新中断：${message}` }
        : item,
    );
  }

  async function runRefresh() {
    if (disposed) return;
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
    options.setRefreshInProgress?.(true);

    // Bound a silent IPC/backend stall without imposing a total time limit on
    // large batches. Each genuine progress event renews the waiting period.
    let timeoutId: ReturnType<typeof setTimeout> | undefined;
    let rejectWait!: (error: Error) => void;
    const timeout = new Promise<never>((_resolve, reject) => {
      rejectWait = reject;
    });
    const clearDeadline = () => globalThis.clearTimeout(timeoutId);
    const renewDeadline = () => {
      clearDeadline();
      timeoutId = globalThis.setTimeout(
        () => rejectWait(new Error("刷新进度长时间未更新，请重试")),
        180_000,
      );
    };
    cancelWait = () => {
      clearDeadline();
      rejectWait(new Error("刷新界面已关闭"));
    };
    renewDeadline();

    try {
      const channel = new Channel<ProviderBatchProgressEvent>((event) => {
        if (
          disposed ||
          sequence !== runSequence ||
          !running.value ||
          event.data.operation !== operation.value
        )
          return;
        renewDeadline();
        handleEvent(event);
      });
      const result = await Promise.race([
        refreshAllProvidersWithProgress(channel),
        timeout,
      ]);
      if (sequence !== runSequence) return;
      options.upsertProviders(result.updatedProviders);
      // The command response may arrive before the channel finishes delivering
      // large provider views. Its final rows close that race deterministically.
      items.value = result.items;
      completed.value = true;
      // Runtime discovery is independent of provider refresh completion.
      void Promise.resolve()
        .then(() => options.refreshCliRuntime?.())
        .catch(() => {});
    } catch (cause) {
      if (sequence !== runSequence) return;
      completed.value = false;
      error.value = cause instanceof Error ? cause.message : String(cause);
      markCommandFailure(error.value);
      Message.error(`刷新失败：${error.value}`);
    } finally {
      clearDeadline();
      if (sequence === runSequence) {
        cancelWait = undefined;
        releaseRefreshing();
        running.value = false;
        finishedAt.value = Date.now();
        options.setRefreshInProgress?.(false);
      }
    }
  }

  onScopeDispose(() => {
    disposed = true;
    runSequence++;
    cancelWait?.();
    cancelWait = undefined;
    releaseRefreshing();
    running.value = false;
    visible.value = false;
    options.setRefreshInProgress?.(false);
  });

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
