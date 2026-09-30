import { computed, onBeforeUnmount, ref, type Ref } from "vue";
import {
  beginCardDrag,
  clearCardDragState,
  createCardDragState,
  cardDragStyleFromState,
  shouldIgnoreCardDragTarget,
} from "../utils/card-drag-state.ts";
import {
  clamp,
  copyRect,
  getCardDragTarget,
  mergeVisibleCardOrder,
  sameCardOrder,
  type DragLayoutItem,
} from "../utils/card-drag-geometry.ts";

interface DragSortOptions<T> {
  items: Ref<T[]>;
  getId: (item: T) => string;
  gridSelector: string;
  dataId: string;
  /** 拖拽隔离分组；分区布局下只允许同组内排序。 */
  dragGroup?: (item: T) => string;
  /** 拖拽结束后持久化新顺序。 */
  reorder: (ids: string[]) => Promise<unknown>;
  /** 持久化失败时的回调。 */
  onError?: (error: unknown) => void;
}

export function useCardDragSort<T>(options: DragSortOptions<T>) {
  const { items, getId } = options;

  const overId = ref<string | null>(null);
  const dragOrder = ref<string[]>([]);
  const clickSuppressed = ref(false);
  let previewFrame: number | null = null;
  let commitTimer: number | null = null;
  let clickReleaseTimer: number | null = null;
  let pendingTargetIndex: number | null = null;
  let pendingTargetSince = 0;
  let dragLayoutSnapshot: DragLayoutItem[] = [];
  let dragSourceElement: HTMLElement | null = null;
  let dragRevision = 0;
  let disposed = false;

  const DRAG_REORDER_DELAY_MS = 110;

  const state = createCardDragState();

  const orderedGroups = computed(() => {
    const groups = new Map<string, T[]>();
    const itemById = new Map(items.value.map((item) => [getId(item), item]));
    const order = mergeVisibleCardOrder([...itemById.keys()], dragOrder.value);
    for (const id of order) {
      const item = itemById.get(id)!;
      const group = itemGroup(item);
      const groupItems = groups.get(group) ?? [];
      groupItems.push(item);
      groups.set(group, groupItems);
    }

    return groups;
  });

  const draggedItem = computed(() => {
    if (!state.id || !state.dragging) {
      return null;
    }
    return items.value.find((item) => getId(item) === state.id) ?? null;
  });

  function handlePointerDown(item: T, event: PointerEvent) {
    if (disposed || state.id) {
      return;
    }
    if (event.button !== 0) {
      return;
    }

    if (shouldIgnoreCardDragTarget(event.target)) {
      return;
    }

    if (!(event.currentTarget instanceof HTMLElement)) {
      return;
    }

    const rect = event.currentTarget.getBoundingClientRect();
    const group = options.dragGroup?.(item) ?? "";
    dragRevision++;
    dragSourceElement = event.currentTarget;
    dragLayoutSnapshot = captureDragLayoutSnapshot(event.currentTarget, group, getId(item), rect);
    overId.value = null;
    dragOrder.value = dragLayoutSnapshot.map((item) => item.id);
    if (clickReleaseTimer !== null) {
      window.clearTimeout(clickReleaseTimer);
      clickReleaseTimer = null;
    }
    clickSuppressed.value = false;
    beginCardDrag(state, {
      currentX: event.clientX,
      currentY: event.clientY,
      group,
      height: rect.height,
      offsetX: event.clientX - rect.left,
      offsetY: event.clientY - rect.top,
      id: getId(item),
      width: rect.width,
    });
    pendingTargetIndex = null;
    pendingTargetSince = 0;
    window.addEventListener("pointermove", handlePointerMove, { passive: false });
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
    window.addEventListener("blur", handlePointerCancel);
    window.addEventListener("resize", handlePointerCancel);
    window.addEventListener("keydown", handleKeyDown);
  }

  function handlePointerMove(event: PointerEvent) {
    if (!state.id) {
      return;
    }

    state.currentX = event.clientX;
    state.currentY = event.clientY;

    const distance = Math.hypot(
      state.currentX - state.startX,
      state.currentY - state.startY,
    );
    if (!state.dragging && distance > 4) {
      state.dragging = true;
      clickSuppressed.value = true;
      document.body.classList.add("workspace-card-drag-active");
    }

    if (!state.dragging) {
      return;
    }

    event.preventDefault();
    scheduleDragPreviewUpdate();
  }

  function scheduleDragPreviewUpdate() {
    if (previewFrame !== null) {
      return;
    }

    previewFrame = window.requestAnimationFrame(() => {
      previewFrame = null;
      updateDragPreviewFromPosition();
    });
  }

  function updateDragPreviewFromPosition(forceCommit = false) {
    const sourceId = state.id;
    if (!sourceId) {
      return;
    }
    const sourceRect = dragSourceElement?.getBoundingClientRect();
    if (!dragSourceElement?.isConnected || !sourceRect?.width || !sourceRect.height) {
      reset(true);
      return;
    }

    const currentOrder = currentOrderIds();
    const orderWithoutSource = currentOrder.filter((id) => id !== sourceId);

    if (orderWithoutSource.length === 0) {
      dragOrder.value = [sourceId];
      overId.value = null;
      return;
    }

    const target = getCardDragTarget(
      orderWithoutSource,
      sourceId,
      state,
      dragLayoutSnapshot,
    );
    const nextIndex = target.index;
    const currentIndex = clamp(currentOrder.indexOf(sourceId), 0, orderWithoutSource.length);
    overId.value =
      target.overId ?? orderWithoutSource[Math.min(nextIndex, orderWithoutSource.length - 1)] ?? null;

    if (nextIndex !== currentIndex && !forceCommit && !targetIndexReady(nextIndex)) {
      return;
    }

    const nextOrder = [...orderWithoutSource];
    nextOrder.splice(nextIndex, 0, sourceId);

    pendingTargetIndex = null;
    pendingTargetSince = 0;
    if (!sameCardOrder(nextOrder, dragOrder.value)) {
      dragOrder.value = nextOrder;
    }
  }

  function targetIndexReady(targetIndex: number) {
    const now = performance.now();
    if (pendingTargetIndex !== targetIndex) {
      pendingTargetIndex = targetIndex;
      pendingTargetSince = now;
      scheduleDragCommit(DRAG_REORDER_DELAY_MS);
      return false;
    }

    const elapsed = now - pendingTargetSince;
    if (elapsed >= DRAG_REORDER_DELAY_MS) {
      return true;
    }

    scheduleDragCommit(DRAG_REORDER_DELAY_MS - elapsed);
    return false;
  }

  function scheduleDragCommit(delayMs: number) {
    if (commitTimer !== null) {
      return;
    }

    commitTimer = window.setTimeout(() => {
      commitTimer = null;
      if (state.dragging) {
        scheduleDragPreviewUpdate();
      }
    }, Math.max(20, delayMs));
  }

  function currentOrderIds() {
    if (dragOrder.value.length > 0) {
      return [...dragOrder.value];
    }
    return dragLayoutSnapshot.map((item) => item.id);
  }

  function itemGroup(item: T) {
    return options.dragGroup?.(item) ?? "";
  }

  function captureDragLayoutSnapshot(source: HTMLElement, group: string, sourceId: string, sourceRect: DOMRect): DragLayoutItem[] {
    const allowedIds = new Set(items.value
      .filter((item) => itemGroup(item) === group)
      .map((item) => getId(item)));
    const grid = source.closest<HTMLElement>(options.gridSelector);
    const snapshot: DragLayoutItem[] = [];
    for (const element of grid?.querySelectorAll<HTMLElement>(":scope > .workspace-card") ?? []) {
      const id = element.dataset[options.dataId];
      if (!id || !allowedIds.has(id)) {
        continue;
      }
      const rect = element.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) snapshot.push({ id, rect: copyRect(rect) });
    }
    if (!snapshot.some((item) => item.id === sourceId)) {
      snapshot.push({ id: sourceId, rect: copyRect(sourceRect) });
    }
    return snapshot;
  }

  function handlePointerUp() {
    flushDragPreviewUpdate();
    const wasDragging = state.dragging;
    const nextVisibleOrder = [...dragOrder.value];
    const nextOrder = mergeVisibleCardOrder(items.value.map((item) => getId(item)), nextVisibleOrder);
    const shouldPersistOrder =
      wasDragging &&
      nextVisibleOrder.length > 0 &&
      !sameCardOrder(
        nextOrder,
        items.value.map((item) => getId(item)),
      );
    reset(wasDragging, shouldPersistOrder);

    if (!shouldPersistOrder) {
      return;
    }

    const expectedRevision = dragRevision;
    void options
      .reorder(nextOrder)
      .catch((error) => {
        if (!disposed && expectedRevision === dragRevision) options.onError?.(error);
      })
      .finally(() => {
        if (!disposed && expectedRevision === dragRevision) dragOrder.value = [];
      });
  }

  function handlePointerCancel() {
    reset(state.dragging);
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    handlePointerCancel();
  }

  function reset(suppressClick: boolean, preserveDragOrder = false) {
    cancelDragPreviewUpdate();
    window.removeEventListener("pointermove", handlePointerMove);
    window.removeEventListener("pointerup", handlePointerUp);
    window.removeEventListener("pointercancel", handlePointerCancel);
    window.removeEventListener("blur", handlePointerCancel);
    window.removeEventListener("resize", handlePointerCancel);
    window.removeEventListener("keydown", handleKeyDown);
    document.body.classList.remove("workspace-card-drag-active");
    overId.value = null;
    clearCardDragState(state);
    dragLayoutSnapshot = [];
    dragSourceElement = null;
    if (!preserveDragOrder) {
      dragRevision++;
      dragOrder.value = [];
    }
    if (clickReleaseTimer !== null) {
      window.clearTimeout(clickReleaseTimer);
      clickReleaseTimer = null;
    }
    clickSuppressed.value = suppressClick && !disposed;
    if (!disposed) {
      clickReleaseTimer = window.setTimeout(() => {
        clickReleaseTimer = null;
        clickSuppressed.value = false;
      }, suppressClick ? 180 : 0);
    }
  }

  function flushDragPreviewUpdate() {
    if (previewFrame !== null) {
      window.cancelAnimationFrame(previewFrame);
      previewFrame = null;
    }
    if (state.dragging) {
      updateDragPreviewFromPosition(true);
    }
  }

  function cancelDragPreviewUpdate() {
    if (previewFrame !== null) {
      window.cancelAnimationFrame(previewFrame);
      previewFrame = null;
    }
    cancelDragCommit();
  }

  function cancelDragCommit() {
    if (commitTimer === null) {
      return;
    }

    window.clearTimeout(commitTimer);
    commitTimer = null;
  }

  const dragStyle = () => cardDragStyleFromState(state);

  onBeforeUnmount(() => {
    disposed = true;
    reset(false);
  });

  return {
    state,
    overId,
    clickSuppressed,
    orderedGroups,
    draggedItem,
    handlePointerDown,
    dragStyle,
    reset,
  };
}
