import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import type { AgentAssetCatalog, AgentCatalogRelationIntent, AgentCatalogRelationPreview, AgentCatalogRelationCapability } from "../stores/agent-catalog-types";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";

export function useAgentCatalogRelations(options: {
  catalog: Ref<AgentAssetCatalog | null>; workspace: Ref<string | undefined>;
  afterMutation: (workspace?: string) => Promise<unknown>;
}) {
  const store = useAgentCatalogStore();
  const visible = ref(false);
  const preview = shallowRef<AgentCatalogRelationPreview | null>(null);
  const intent = shallowRef<AgentCatalogRelationIntent | null>(null);
  const comparing = shallowRef<Extract<AgentCatalogRelationIntent, { kind: "compare" }> | null>(null);
  const loading = ref(false);
  const error = ref("");
  const now = ref(Date.now());
  let request = 0;
  let disposed = false;
  let timer: ReturnType<typeof globalThis.setInterval> | null = null;
  const expired = computed(() => Boolean(preview.value?.token && (!preview.value.expiresAt || !Number.isFinite(Date.parse(preview.value.expiresAt)) || Date.parse(preview.value.expiresAt) <= now.value)));
  const canConfirm = computed(() => Boolean(preview.value?.token && preview.value.available && preview.value.action !== "compare" && !loading.value && !expired.value));
  function close() {
    request += 1; visible.value = false; preview.value = null; intent.value = null; comparing.value = null; loading.value = false; error.value = "";
    if (timer !== null) globalThis.clearInterval(timer);
    timer = null;
  }
  async function load(next: AgentCatalogRelationIntent) {
    if (!options.catalog.value || disposed) return;
    const workspace = options.workspace.value;
    const current = ++request;
    visible.value = true; intent.value = next; preview.value = null; loading.value = true; error.value = "";
    if (timer !== null) globalThis.clearInterval(timer);
    try {
      const catalog = await store.ensureReady(workspace);
      if (disposed || current !== request || workspace !== options.workspace.value) return;
      const result = await store.previewRelation({ intent: next, expectedRevision: catalog.revision, workspace: workspace ?? null });
      if (disposed || current !== request || workspace !== options.workspace.value) return;
      if (catalog.revision !== options.catalog.value?.revision) throw new Error("目录在读取期间发生变化，请重新读取当前状态");
      if (result.action !== next.kind) throw new Error("返回的整理计划与所选操作不一致");
      preview.value = result; now.value = Date.now();
      timer = result.token ? globalThis.setInterval(() => { now.value = Date.now(); }, 1_000) : null;
    } catch (failure) { if (!disposed && current === request) error.value = agentEnvironmentErrorMessage(failure); }
    finally { if (current === request) loading.value = false; }
  }
  watch(() => options.catalog.value?.revision, (next, previous) => {
    if (!visible.value || loading.value || !previous || next === previous) return;
    preview.value = null;
    error.value = "目录已更新，请重新读取当前资源关系";
    if (timer !== null) globalThis.clearInterval(timer);
    timer = null;
  });
  function open(next: AgentCatalogRelationIntent) { close(); comparing.value = next.kind === "compare" ? next : null; return load(next); }
  function choose(capability: AgentCatalogRelationCapability) {
    if (!capability.available || !preview.value?.capabilities.some((entry) => entry.available && JSON.stringify(entry.intent) === JSON.stringify(capability.intent))) return;
    return load(capability.intent);
  }
  function back() { if (comparing.value) return load(comparing.value); }
  function retry() { if (intent.value) return load(intent.value); }
  function confirm() {
    now.value = Date.now();
    const result = preview.value;
    if (!result || !canConfirm.value) return;
    const workspace = options.workspace.value;
    close();
    void store.commitRelation(result, workspace).then((submitted) => submitted ? options.afterMutation(workspace) : undefined);
  }
  onScopeDispose(() => { disposed = true; close(); });
  return { visible, preview, intent, loading, error, expired, canConfirm, canBack: computed(() => Boolean(comparing.value && intent.value?.kind !== "compare")), open, choose, back, retry, close, confirm };
}
