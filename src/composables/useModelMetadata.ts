import {
  computed,
  getCurrentInstance,
  onMounted,
  onScopeDispose,
  ref,
  watch,
  type Ref,
} from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  getModelMetadata,
  type ModelMetadataSnapshot,
} from "../api/model-catalog";
import { withTimeout } from "../utils/promise-timeout";

export function useModelMetadata(ids: Ref<string[]>, active: Ref<boolean>) {
  const snapshot = ref<ModelMetadataSnapshot | null>(null);
  const loading = ref(false);
  const error = ref("");
  const models = computed(
    () =>
      new Map(snapshot.value?.models.map((model) => [model.id, model]) ?? []),
  );
  let revision = 0;
  let disposed = false;
  let unlisten: UnlistenFn | null = null;
  async function load(refresh = false) {
    const current = ++revision;
    if (!active.value || !ids.value.length) {
      loading.value = false;
      return;
    }
    loading.value = true;
    error.value = "";
    try {
      const result = await withTimeout(
        getModelMetadata([...new Set(ids.value)], refresh),
        35_000,
        "读取模型资料超时",
      );
      if (!disposed && current === revision) {
        snapshot.value = result;
        error.value = result.error ?? "";
      }
    } catch (e) {
      if (!disposed && current === revision)
        error.value = typeof e === "string" ? e : "模型资料暂不可用";
    } finally {
      if (current === revision) loading.value = false;
    }
  }
  watch(
    [ids, active],
    () => {
      void load();
    },
    { immediate: true },
  );
  if (getCurrentInstance())
    onMounted(async () => {
      try {
        const stop = await listen("model-metadata-updated", () => {
          if (active.value && !loading.value) void load();
        });
        if (disposed) stop();
        else unlisten = stop;
      } catch {
        /* The native event bus is unavailable in isolated components. */
      }
    });
  onScopeDispose(() => {
    disposed = true;
    revision += 1;
    unlisten?.();
  });
  return { models, snapshot, loading, error, refresh: () => load(true) };
}
