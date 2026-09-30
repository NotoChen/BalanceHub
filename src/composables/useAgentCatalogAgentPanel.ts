import { onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import type { AgentAssetCatalog, AgentCatalogAgentPanel } from "../stores/agent-catalog-types";
import type { AgentCliKind } from "../stores/provider-types";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";
import { cancelAgentCatalogRead } from "../api/agent-catalog";

export interface AgentCatalogAgentSelection { assetId: string; agentKind: AgentCliKind | null }
export function useAgentCatalogAgentPanel(options: { catalog: Ref<AgentAssetCatalog | null>; workspace: Ref<string | undefined> }) {
  const store = useAgentCatalogStore();
  const selection = shallowRef<AgentCatalogAgentSelection | null>(null);
  const panel = shallowRef<AgentCatalogAgentPanel | null>(null);
  const loading = ref(false);
  const error = ref("");
  let request = 0;
  let disposed = false;
  let activeId: string | null = null;
  function cancelRead() { if (activeId) void cancelAgentCatalogRead(activeId).catch(() => {}); activeId = null; }
  function close() { request += 1; cancelRead(); selection.value = null; panel.value = null; loading.value = false; error.value = ""; }
  async function open(assetId: string, agentKind: AgentCliKind | null) {
    close();
    if (!options.catalog.value) return;
    const current = ++request;
    const requestId = crypto.randomUUID();
    activeId = requestId;
    const workspace = options.workspace.value ?? null;
    selection.value = { assetId, agentKind }; loading.value = true;
    try {
      const catalog = await store.ensureReady(workspace ?? undefined);
      if (disposed || current !== request || workspace !== (options.workspace.value ?? null)) return;
      const result = await store.agentPanel({ assetId, agentKind, expectedRevision: catalog.revision, workspace }, requestId);
      if (disposed || current !== request || workspace !== (options.workspace.value ?? null)) return;
      if (catalog.revision !== options.catalog.value?.revision) throw new Error("目录在读取期间发生变化，请重新读取当前状态");
      if (result.assetId !== assetId || result.agentKind !== agentKind || result.revision !== catalog.revision) throw new Error("使用详情已变化，请刷新目录后重试");
      panel.value = result;
    } catch (failure) { if (!disposed && current === request) { cancelRead(); error.value = agentEnvironmentErrorMessage(failure); } }
    finally { if (current === request) { loading.value = false; activeId = null; } }
  }
  function retry() { const current = selection.value; if (current) return open(current.assetId, current.agentKind); }
  watch(() => options.catalog.value?.revision, (next) => {
    const currentRevision = panel.value?.revision;
    if (selection.value && panel.value && !loading.value && currentRevision !== next) void retry();
  });
  onScopeDispose(() => { disposed = true; close(); });
  return { selection, panel, loading, error, open, close, retry };
}
