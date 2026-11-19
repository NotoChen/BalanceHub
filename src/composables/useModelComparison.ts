import { onScopeDispose, ref } from "vue";
import { getModelMetadata } from "../api/model-catalog";
import type { ModelMetadata } from "../api/model-catalog";
import type { Provider } from "../stores/provider-types";
import {
  buildModelComparisonRows,
  modelComparisonProviders,
  type ModelComparisonRow,
} from "../utils/model-comparison";
import { withTimeout } from "../utils/promise-timeout";

const COMPARISON_TIMEOUT_MS = 35_000;

function reason(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * 跨站比价：只读取各卡片已有的站点价目和卡片覆盖价，不触发新的站点请求。
 * 每个中转站单独请求，单站失败只影响对应行。
 */
export function useModelComparison(options: {
  providers: () => readonly Provider[];
  currentProviderId: () => string | undefined;
}) {
  const model = ref("");
  const rows = ref<ModelComparisonRow[]>([]);
  const loading = ref(false);
  const error = ref("");
  let revision = 0;

  function close() {
    revision += 1;
    model.value = "";
    rows.value = [];
    loading.value = false;
    error.value = "";
  }

  async function open(nextModel: string) {
    const name = nextModel.trim();
    if (!name) return;
    const current = ++revision;
    const sameModel = model.value === name;
    model.value = name;
    if (!sameModel) rows.value = [];
    loading.value = true;
    error.value = "";
    const targets = modelComparisonProviders(options.providers(), name);
    if (!targets.length) {
      loading.value = false;
      return;
    }
    try {
      const results = await Promise.all(
        targets.map(async (provider) => {
          try {
            const snapshot = await withTimeout(
              getModelMetadata([name], false, provider.identity.id),
              COMPARISON_TIMEOUT_MS,
              "读取比价资料超时",
            );
            const metadata: ModelMetadata | null = snapshot.models[0] ?? null;
            return {
              provider,
              metadata,
              error: metadata ? "" : (snapshot.error ?? ""),
            };
          } catch (cause) {
            return { provider, metadata: null, error: reason(cause) };
          }
        }),
      );
      if (current !== revision) return;
      rows.value = buildModelComparisonRows(
        name,
        results,
        options.currentProviderId(),
      );
      if (rows.value.every((row) => !row.metadata)) {
        error.value = "没有读取到任何中转站的模型资料";
      }
    } catch (cause) {
      if (current === revision) {
        error.value = reason(cause);
        rows.value = [];
      }
    } finally {
      if (current === revision) loading.value = false;
    }
  }

  onScopeDispose(close);

  return { model, rows, loading, error, open, close };
}
