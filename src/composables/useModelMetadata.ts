import {
  computed,
  getCurrentInstance,
  onMounted,
  onScopeDispose,
  ref,
  watch,
  type Ref,
} from "vue";
import { listen, type UnlistenFn } from "../api/transport.ts";
import {
  getModelMetadata,
  type ModelMetadataSnapshot,
} from "../api/model-catalog";
import { withTimeout } from "../utils/promise-timeout";

export function useModelMetadata(
  ids: Ref<string[]>,
  active: Ref<boolean>,
  providerId?: Ref<string | undefined>,
  credentialRevision?: Ref<number | undefined>,
) {
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
        getModelMetadata([...new Set(ids.value)], refresh, providerId?.value),
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
    [
      ids,
      active,
      ...(providerId ? [providerId] : []),
      ...(credentialRevision ? [credentialRevision] : []),
    ],
    () => {
      void load();
    },
    { immediate: true },
  );
  // 快照按模型名索引，站点价目却属于具体卡片和当前凭据。切换卡片、换 Key
  // 或关闭窗口时先清空，不能让上一张卡/上一个 Key 的分组价在等待新结果时
  // 显示在新卡片的同名模型上；分页只改变请求范围，仍沿用同一份资料。
  watch(
    () => [providerId?.value, credentialRevision?.value] as const,
    ([nextProvider, nextRevision], [previousProvider, previousRevision]) => {
      if (nextProvider === previousProvider && nextRevision === previousRevision) {
        return;
      }
      snapshot.value = null;
      error.value = "";
    },
  );
  watch(active, (value) => {
    if (value) return;
    snapshot.value = null;
    error.value = "";
  });
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
