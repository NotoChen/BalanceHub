import { computed, onScopeDispose, shallowReactive, watch } from "vue";
import { getAgentModelCatalog } from "../api/agent-models";
import type {
  AgentModelCatalog,
  AgentModelCatalogContext,
} from "../stores/agent-settings-types";
import { withTimeout } from "../utils/promise-timeout";

interface CatalogEntry {
  value: AgentModelCatalog | null;
  loading: boolean;
  error: string;
  pending: Promise<void> | null;
}
// Model fields in the same form share a small display cache and one IPC query.
// Freshness and installation/account invalidation remain backend decisions.
const entries = new Map<string, CatalogEntry>();

export function useAgentModelCatalog(
  context: () => AgentModelCatalogContext | undefined,
) {
  const key = computed(() => (context() ? JSON.stringify(context()) : ""));
  const entry = computed(() => {
    if (!key.value) return undefined;
    let current = entries.get(key.value);
    if (!current) {
      if (entries.size >= 32) {
        for (const [oldKey, oldEntry] of entries) {
          if (!oldEntry.pending) {
            entries.delete(oldKey);
            break;
          }
        }
      }
      current = shallowReactive<CatalogEntry>({
        value: null,
        loading: false,
        error: "",
        pending: null,
      });
      entries.set(key.value, current);
    }
    return current;
  });
  let timer: ReturnType<typeof setInterval> | undefined;

  function load(refresh = false): Promise<void> {
    const request = context();
    const current = entry.value;
    if (!request || !current) return Promise.resolve();
    if (current.pending) return current.pending;
    current.loading = true;
    current.error = "";
    // Completion updates only the cache for this stable context key. A closed
    // picker or a different CLI never receives another context's late result.
    current.pending = withTimeout(
      getAgentModelCatalog({ ...request }, refresh),
      20_000,
      "模型目录刷新超时，仍可使用已有模型或输入模型 ID",
    )
      .then((result) => {
        current.value = result;
      })
      .catch((cause: unknown) => {
        current.error = cause instanceof Error ? cause.message : String(cause);
      })
      .finally(() => {
        current.loading = false;
        current.pending = null;
      });
    return current.pending;
  }

  function popupVisible(visible: boolean) {
    clearInterval(timer);
    timer = undefined;
    if (!visible) return;
    void load();
    // While the picker stays open, also notice external CLI/account updates.
    // A cache hit performs filesystem metadata checks, not a process or scan.
    timer = setInterval(() => {
      void load();
    }, 60_000);
  }
  watch(
    key,
    () => {
      void load();
    },
    { immediate: true },
  );
  onScopeDispose(() => clearInterval(timer));
  return {
    catalog: computed(() => entry.value?.value ?? null),
    loading: computed(() => entry.value?.loading ?? false),
    notice: computed(
      () => entry.value?.error || entry.value?.value?.notice || "",
    ),
    load,
    popupVisible,
  };
}
