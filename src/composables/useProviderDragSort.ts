import { computed, ref, type Ref } from "vue";
import type { Provider } from "../stores/providers";
import {
  beginProviderDrag,
  clearProviderDragState,
  createProviderDragState,
  providerDragStyleFromState,
  shouldIgnoreProviderDragTarget,
} from "../utils/provider-drag-state";
import {
  clamp,
  copyRect,
  getProviderDragTarget,
  sameProviderOrder,
  type DragLayoutItem,
  type DragRect,
} from "../utils/provider-drag-geometry";

interface DragSortOptions {
  /** 当前中转站列表。 */
  providers: Ref<Provider[]>;
  /** 拖拽隔离分组；分区布局下只允许同组内排序。 */
  dragGroup?: (provider: Provider) => string;
  /** 拖拽结束后持久化新顺序。 */
  reorder: (ids: string[]) => Promise<unknown>;
  /** 拖拽开始时的回调（例如关闭右键菜单）。 */
  onDragStart?: () => void;
  /** 持久化失败时的回调。 */
  onError?: (error: unknown) => void;
}

export function useProviderDragSort(options: DragSortOptions) {
  const { providers } = options;

  const draggingProviderId = ref<string | null>(null);
  const dragOverProviderId = ref<string | null>(null);
  const dragOrder = ref<string[]>([]);
  const dragOrderGroup = ref("");
  const providerCardClickSuppressed = ref(false);
  let providerDragPreviewFrame: number | null = null;
  let providerDragCommitTimer: number | null = null;
  let pendingTargetIndex: number | null = null;
  let pendingTargetSince = 0;
  let dragLayoutSnapshot: DragLayoutItem[] = [];

  const DRAG_REORDER_DELAY_MS = 110;

  const providerDrag = createProviderDragState();

  const orderedProviderGroups = computed(() => {
    const groups = new Map<string, Provider[]>();
    for (const provider of providers.value) {
      const group = providerDragGroup(provider);
      const items = groups.get(group) ?? [];
      items.push(provider);
      groups.set(group, items);
    }

    if (dragOrder.value.length === 0 || !dragOrderGroup.value) {
      return groups;
    }

    const activeGroupProviders = groups.get(dragOrderGroup.value) ?? [];
    const providerById = new Map(activeGroupProviders.map((provider) => [provider.identity.id, provider]));
    const orderedGroup = dragOrder.value
      .map((id) => providerById.get(id))
      .filter((provider): provider is Provider => Boolean(provider));
    const orderedIds = new Set(orderedGroup.map((provider) => provider.identity.id));
    const missing = activeGroupProviders.filter((provider) => !orderedIds.has(provider.identity.id));
    groups.set(dragOrderGroup.value, [...orderedGroup, ...missing]);
    return groups;
  });

  const draggedProvider = computed(() => {
    if (!providerDrag.providerId || !providerDrag.dragging) {
      return null;
    }
    return providers.value.find((provider) => provider.identity.id === providerDrag.providerId) ?? null;
  });

  function handleProviderPointerDown(provider: Provider, event: PointerEvent) {
    if (event.button !== 0) {
      return;
    }

    if (shouldIgnoreProviderDragTarget(event.target)) {
      return;
    }

    if (!(event.currentTarget instanceof HTMLElement)) {
      return;
    }

    const rect = event.currentTarget.getBoundingClientRect();
    options.onDragStart?.();
    const group = options.dragGroup?.(provider) ?? "";
    draggingProviderId.value = provider.identity.id;
    dragOverProviderId.value = null;
    dragOrderGroup.value = group;
    dragOrder.value = providers.value
      .filter((item) => providerDragGroup(item) === group)
      .map((item) => item.identity.id);
    providerCardClickSuppressed.value = false;
    beginProviderDrag(providerDrag, {
      currentX: event.clientX,
      currentY: event.clientY,
      group,
      height: rect.height,
      offsetX: event.clientX - rect.left,
      offsetY: event.clientY - rect.top,
      providerId: provider.identity.id,
      width: rect.width,
    });
    dragLayoutSnapshot = captureDragLayoutSnapshot(group, provider.identity.id, rect);
    pendingTargetIndex = null;
    pendingTargetSince = 0;
    window.addEventListener("pointermove", handleProviderPointerMove, { passive: false });
    window.addEventListener("pointerup", handleProviderPointerUp);
    window.addEventListener("pointercancel", handleProviderPointerCancel);
  }

  function handleProviderPointerMove(event: PointerEvent) {
    if (!providerDrag.providerId) {
      return;
    }

    providerDrag.currentX = event.clientX;
    providerDrag.currentY = event.clientY;

    const distance = Math.hypot(
      providerDrag.currentX - providerDrag.startX,
      providerDrag.currentY - providerDrag.startY,
    );
    if (!providerDrag.dragging && distance > 4) {
      providerDrag.dragging = true;
      providerCardClickSuppressed.value = true;
      document.body.classList.add("provider-drag-active");
    }

    if (!providerDrag.dragging) {
      return;
    }

    event.preventDefault();
    scheduleProviderDragPreviewUpdate();
  }

  function scheduleProviderDragPreviewUpdate() {
    if (providerDragPreviewFrame !== null) {
      return;
    }

    providerDragPreviewFrame = window.requestAnimationFrame(() => {
      providerDragPreviewFrame = null;
      updateProviderDragPreviewFromPosition();
    });
  }

  function updateProviderDragPreviewFromPosition(forceCommit = false) {
    const sourceId = providerDrag.providerId;
    if (!sourceId) {
      return;
    }

    const currentOrder = currentProviderOrder();
    const orderWithoutSource = currentOrder.filter((id) => id !== sourceId);

    if (orderWithoutSource.length === 0) {
      dragOrder.value = [sourceId];
      dragOverProviderId.value = null;
      return;
    }

    const target = getProviderDragTarget(
      orderWithoutSource,
      sourceId,
      providerDrag,
      dragLayoutSnapshot,
    );
    const nextIndex = target.index;
    const currentIndex = clamp(currentOrder.indexOf(sourceId), 0, orderWithoutSource.length);
    dragOverProviderId.value =
      target.overId ?? orderWithoutSource[Math.min(nextIndex, orderWithoutSource.length - 1)] ?? null;

    if (nextIndex !== currentIndex && !forceCommit && !targetIndexReady(nextIndex)) {
      return;
    }

    const nextOrder = [...orderWithoutSource];
    nextOrder.splice(nextIndex, 0, sourceId);

    pendingTargetIndex = null;
    pendingTargetSince = 0;
    if (!sameProviderOrder(nextOrder, dragOrder.value)) {
      dragOrder.value = nextOrder;
    }
  }

  function targetIndexReady(targetIndex: number) {
    const now = performance.now();
    if (pendingTargetIndex !== targetIndex) {
      pendingTargetIndex = targetIndex;
      pendingTargetSince = now;
      scheduleProviderDragCommit(DRAG_REORDER_DELAY_MS);
      return false;
    }

    const elapsed = now - pendingTargetSince;
    if (elapsed >= DRAG_REORDER_DELAY_MS) {
      return true;
    }

    scheduleProviderDragCommit(DRAG_REORDER_DELAY_MS - elapsed);
    return false;
  }

  function scheduleProviderDragCommit(delayMs: number) {
    if (providerDragCommitTimer !== null) {
      return;
    }

    providerDragCommitTimer = window.setTimeout(() => {
      providerDragCommitTimer = null;
      if (providerDrag.dragging) {
        scheduleProviderDragPreviewUpdate();
      }
    }, Math.max(20, delayMs));
  }

  function currentProviderOrder() {
    if (dragOrder.value.length > 0) {
      return [...dragOrder.value];
    }
    return providers.value
      .filter((provider) => providerDragGroup(provider) === providerDrag.group)
      .map((provider) => provider.identity.id);
  }

  function providerDragGroup(provider: Provider) {
    return options.dragGroup?.(provider) ?? "";
  }

  function captureDragLayoutSnapshot(group: string, sourceId: string, sourceRect: DOMRect): DragLayoutItem[] {
    const order = providers.value
      .filter((provider) => providerDragGroup(provider) === group)
      .map((provider) => provider.identity.id);
    const rectById = new Map<string, DragRect>();
    for (const element of document.querySelectorAll<HTMLElement>(".overview-provider-grid [data-provider-id]")) {
      const id = element.dataset.providerId;
      if (!id || !order.includes(id)) {
        continue;
      }
      rectById.set(id, copyRect(element.getBoundingClientRect()));
    }
    if (!rectById.has(sourceId)) {
      rectById.set(sourceId, copyRect(sourceRect));
    }
    return order
      .map((id) => {
        const rect = rectById.get(id);
        return rect ? { id, rect } : null;
      })
      .filter((item): item is DragLayoutItem => Boolean(item));
  }

  function handleProviderPointerUp() {
    flushProviderDragPreviewUpdate();
    const wasDragging = providerDrag.dragging;
    const nextGroupOrder = dragOrder.value.length > 0 ? [...dragOrder.value] : [];
    const nextOrder = mergeGroupOrder(nextGroupOrder, providerDrag.group);
    const shouldPersistOrder =
      wasDragging &&
      nextGroupOrder.length > 0 &&
      !sameProviderOrder(
        nextOrder,
        providers.value.map((provider) => provider.identity.id),
      );
    resetProviderPointerDrag(wasDragging, shouldPersistOrder);

    if (!shouldPersistOrder) {
      return;
    }

    void options
      .reorder(nextOrder)
      .catch((error) => {
        options.onError?.(error);
      })
      .finally(() => {
        dragOrder.value = [];
        dragOrderGroup.value = "";
      });
  }

  function handleProviderPointerCancel() {
    resetProviderPointerDrag(providerDrag.dragging);
  }

  function resetProviderPointerDrag(suppressClick: boolean, preserveDragOrder = false) {
    cancelProviderDragPreviewUpdate();
    window.removeEventListener("pointermove", handleProviderPointerMove);
    window.removeEventListener("pointerup", handleProviderPointerUp);
    window.removeEventListener("pointercancel", handleProviderPointerCancel);
    document.body.classList.remove("provider-drag-active");
    draggingProviderId.value = null;
    dragOverProviderId.value = null;
    clearProviderDragState(providerDrag);
    dragLayoutSnapshot = [];
    if (!preserveDragOrder) {
      dragOrder.value = [];
      dragOrderGroup.value = "";
    }
    window.setTimeout(
      () => {
        providerCardClickSuppressed.value = false;
      },
      suppressClick ? 180 : 0,
    );
  }

  function flushProviderDragPreviewUpdate() {
    if (providerDragPreviewFrame !== null) {
      window.cancelAnimationFrame(providerDragPreviewFrame);
      providerDragPreviewFrame = null;
    }
    if (providerDrag.dragging) {
      updateProviderDragPreviewFromPosition(true);
    }
  }

  function cancelProviderDragPreviewUpdate() {
    if (providerDragPreviewFrame !== null) {
      window.cancelAnimationFrame(providerDragPreviewFrame);
      providerDragPreviewFrame = null;
    }
    cancelProviderDragCommit();
  }

  function cancelProviderDragCommit() {
    if (providerDragCommitTimer === null) {
      return;
    }

    window.clearTimeout(providerDragCommitTimer);
    providerDragCommitTimer = null;
  }

  function mergeGroupOrder(groupOrder: string[], group: string) {
    const queue = [...groupOrder];
    return providers.value.map((provider) => {
      if (providerDragGroup(provider) !== group) {
        return provider.identity.id;
      }
      return queue.shift() ?? provider.identity.id;
    });
  }

  const providerDragStyle = () => providerDragStyleFromState(providerDrag);

  return {
    providerDrag,
    draggingProviderId,
    dragOverProviderId,
    providerCardClickSuppressed,
    orderedProviderGroups,
    draggedProvider,
    handleProviderPointerDown,
    providerDragStyle,
    resetProviderPointerDrag,
  };
}
