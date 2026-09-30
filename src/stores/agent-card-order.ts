import { defineStore } from "pinia";
import { shallowRef } from "vue";
import { mergeVisibleCardOrder } from "../utils/card-drag-geometry";

const STORAGE_KEY = "balancehub.agent-card-order";

function loadOrder(): string[] {
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(value)
      ? [...new Set(value.filter((id): id is string => typeof id === "string" && id.length > 0 && id.length <= 128))]
      : [];
  } catch { return []; }
}

/** Local presentation preference; Agent definitions and capabilities stay in Rust. */
export const useAgentCardOrderStore = defineStore("agent-card-order", () => {
  const order = shallowRef(loadOrder());

  function sorted<T extends { kind: string }>(agents: T[]): T[] {
    const ranks = new Map(order.value.map((kind, index) => [kind, index]));
    return [...agents].sort((left, right) => (ranks.get(left.kind) ?? order.value.length)
      - (ranks.get(right.kind) ?? order.value.length));
  }

  async function reorder(currentOrder: string[], visibleOrder: string[]) {
    const next = mergeVisibleCardOrder(currentOrder, visibleOrder);
    try { window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next)); }
    catch { throw new Error("保存 Agent 排序失败，请重试"); }
    order.value = next;
  }

  return { sorted, reorder };
});
