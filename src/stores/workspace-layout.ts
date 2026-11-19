import { defineStore } from "pinia";
import { computed, ref } from "vue";

export type WorkspaceLayout = "cards" | "list";
export type WorkspaceLayoutBoard = "providers" | "agents";

const STORAGE_KEY = "balancehub.workspace-layouts";

function normalizeLayout(value: unknown): WorkspaceLayout {
  return value === "list" ? "list" : "cards";
}

function readStorage(key: string): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function loadLayouts(): Record<WorkspaceLayoutBoard, WorkspaceLayout> {
  const raw = readStorage(STORAGE_KEY);
  let parsed: Partial<Record<WorkspaceLayoutBoard, unknown>> = {};
  if (raw) {
    try {
      const value = JSON.parse(raw);
      if (value && typeof value === "object") parsed = value;
    } catch {
      parsed = {};
    }
  }
  return {
    providers: normalizeLayout(parsed.providers),
    agents: normalizeLayout(parsed.agents),
  };
}

/// 本地展示偏好：只决定卡片以网格还是单行列表出现，不进入站点数据，也不
/// 影响后端能力判断。两个看板各存一份，切换 Agent 布局不改变中转站布局。
export const useWorkspaceLayoutStore = defineStore("workspace-layout", () => {
  const layouts = ref<Record<WorkspaceLayoutBoard, WorkspaceLayout>>(loadLayouts());

  /// 兼容只关心中转站看板的调用方。
  const layout = computed(() => layouts.value.providers);

  function persist() {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(layouts.value));
    } catch {
      // 存储不可用（隐私模式、配额受限）时只保留本次会话的选择。
    }
  }

  function layoutFor(board: WorkspaceLayoutBoard) {
    return layouts.value[board];
  }

  function setBoardLayout(board: WorkspaceLayoutBoard, next: WorkspaceLayout) {
    layouts.value = { ...layouts.value, [board]: normalizeLayout(next) };
    persist();
  }

  function setLayout(next: WorkspaceLayout) {
    setBoardLayout("providers", next);
  }

  return { layout, layouts, layoutFor, setLayout, setBoardLayout };
});
