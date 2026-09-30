import { reactive, type CSSProperties } from "vue";

export interface CardDragState {
  id: string | null;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
  group: string;
  dragging: boolean;
}

interface BeginCardDragOptions {
  currentX: number;
  currentY: number;
  group: string;
  height: number;
  offsetX: number;
  offsetY: number;
  id: string;
  width: number;
}

export function createCardDragState() {
  return reactive<CardDragState>({
    id: null,
    startX: 0,
    startY: 0,
    currentX: 0,
    currentY: 0,
    offsetX: 0,
    offsetY: 0,
    width: 0,
    height: 0,
    group: "",
    dragging: false,
  });
}

export function beginCardDrag(state: CardDragState, options: BeginCardDragOptions) {
  state.id = options.id;
  state.group = options.group;
  state.startX = options.currentX;
  state.startY = options.currentY;
  state.currentX = options.currentX;
  state.currentY = options.currentY;
  state.offsetX = options.offsetX;
  state.offsetY = options.offsetY;
  state.width = options.width;
  state.height = options.height;
  state.dragging = false;
}

export function clearCardDragState(state: CardDragState) {
  state.id = null;
  state.group = "";
  state.dragging = false;
}

export function cardDragStyleFromState(state: CardDragState): CSSProperties {
  if (!state.id || !state.dragging) {
    return {};
  }

  return {
    height: `${state.height}px`,
    left: `${state.currentX - state.offsetX}px`,
    position: "fixed",
    top: `${state.currentY - state.offsetY}px`,
    width: `${state.width}px`,
    zIndex: 2000,
  };
}

export function shouldIgnoreCardDragTarget(target: EventTarget | null) {
  return (
    target instanceof Element &&
    Boolean(target.closest("button, a, input, textarea, select, [role='menuitem']"))
  );
}
