export interface DragRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
  width: number;
  height: number;
}

export interface DragLayoutItem {
  id: string;
  rect: DragRect;
}

export interface DragTargetState {
  currentX: number;
  currentY: number;
  height: number;
  offsetX: number;
  offsetY: number;
  width: number;
}

export interface DragTargetResult {
  index: number;
  overId: string | null;
}

export function getProviderDragTarget(
  orderWithoutSource: string[],
  sourceId: string,
  dragState: DragTargetState,
  layoutSnapshot: DragLayoutItem[],
): DragTargetResult {
  const itemCountWithoutSource = orderWithoutSource.length;
  if (dragState.width <= 0 || dragState.height <= 0) {
    return { index: itemCountWithoutSource, overId: null };
  }

  const draggedRect = {
    left: dragState.currentX - dragState.offsetX,
    top: dragState.currentY - dragState.offsetY,
    right: dragState.currentX - dragState.offsetX + dragState.width,
    bottom: dragState.currentY - dragState.offsetY + dragState.height,
    width: dragState.width,
    height: dragState.height,
  };
  const draggedCenterX = draggedRect.left + draggedRect.width / 2;
  const draggedCenterY = draggedRect.top + draggedRect.height / 2;
  const orderIndexById = new Map(orderWithoutSource.map((id, index) => [id, index]));
  const cards = layoutSnapshot
    .filter((item) => item.id !== sourceId && orderIndexById.has(item.id))
    .sort((left, right) => orderIndexById.get(left.id)! - orderIndexById.get(right.id)!);

  if (cards.length === 0) {
    return { index: itemCountWithoutSource, overId: null };
  }

  const dragArea = draggedRect.width * draggedRect.height;
  const overlapTarget = cards
    .map((card, index) => {
      const area = intersectionArea(draggedRect, card.rect);
      const targetArea = card.rect.width * card.rect.height;
      return {
        card,
        index,
        score: area / Math.max(1, Math.min(dragArea, targetArea)),
      };
    })
    .filter((item) => item.score >= 0.18)
    .sort((left, right) => right.score - left.score)[0];

  if (overlapTarget) {
    return {
      index: insertionIndexFromTarget(overlapTarget.index, draggedRect, overlapTarget.card.rect),
      overId: overlapTarget.card.id,
    };
  }

  for (const [index, card] of cards.entries()) {
    const rowCenter = card.rect.top + card.rect.height / 2;
    const columnCenter = card.rect.left + card.rect.width / 2;

    if (draggedCenterY < card.rect.top) {
      return { index, overId: card.id };
    }

    if (draggedCenterY <= card.rect.bottom) {
      const indexInRow =
        draggedCenterX < columnCenter || draggedCenterY < rowCenter - card.rect.height * 0.18
          ? index
          : index + 1;
      return { index: indexInRow, overId: card.id };
    }
  }

  return { index: itemCountWithoutSource, overId: cards[cards.length - 1]?.id ?? null };
}

export function copyRect(rect: DOMRect): DragRect {
  return {
    left: rect.left,
    right: rect.right,
    top: rect.top,
    bottom: rect.bottom,
    width: rect.width,
    height: rect.height,
  };
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

export function sameProviderOrder(left: string[], right: string[]) {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

function insertionIndexFromTarget(targetIndex: number, draggedRect: DragRect, targetRect: DragRect) {
  const sameRow =
    draggedRect.bottom > targetRect.top + targetRect.height * 0.25 &&
    draggedRect.top < targetRect.bottom - targetRect.height * 0.25;
  if (sameRow) {
    const draggedCenterX = draggedRect.left + draggedRect.width / 2;
    const targetCenterX = targetRect.left + targetRect.width / 2;
    return draggedCenterX < targetCenterX ? targetIndex : targetIndex + 1;
  }

  const draggedCenterY = draggedRect.top + draggedRect.height / 2;
  const targetCenterY = targetRect.top + targetRect.height / 2;
  return draggedCenterY < targetCenterY ? targetIndex : targetIndex + 1;
}

function intersectionArea(
  left: { left: number; right: number; top: number; bottom: number },
  right: { left: number; right: number; top: number; bottom: number },
) {
  const width = Math.max(0, Math.min(left.right, right.right) - Math.max(left.left, right.left));
  const height = Math.max(0, Math.min(left.bottom, right.bottom) - Math.max(left.top, right.top));
  return width * height;
}
