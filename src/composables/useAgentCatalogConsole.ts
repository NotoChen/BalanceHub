import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { agentEnvironmentErrorMessage, agentEnvironmentKey } from "../stores/agent-environment";
import type { AgentAssetCategory, AgentCliKind } from "../stores/provider-types";
import type { AgentCatalogAction, AgentCatalogDefinition, AgentCatalogPlanSource, AgentCatalogSaveRequest } from "../stores/agent-catalog-types";
import { useAgentCatalogPlan } from "./useAgentCatalogPlan";
import { useAgentCatalogAgentPanel } from "./useAgentCatalogAgentPanel";
import { useAgentCatalogRelations } from "./useAgentCatalogRelations";

export type { AgentCatalogTargetChoice } from "./useAgentCatalogPlan";

export function useAgentCatalogConsole(options: { workspace: Ref<string | undefined>; navigationRevision: Ref<number> }) {
  const store = useAgentCatalogStore();
  const catalog = computed(() => store.catalogs[agentEnvironmentKey(options.workspace.value)] ?? null);
  const plans = useAgentCatalogPlan({ catalog, workspace: options.workspace, afterMutation: refreshCatalog });
  const management = useAgentCatalogAgentPanel({ catalog, workspace: options.workspace });
  const relations = useAgentCatalogRelations({ catalog, workspace: options.workspace, afterMutation: refreshCatalog });
  const selectedId = ref<string | null>(null);
  const selectedFeature = ref<string | null>(null);
  const selectedAsset = computed(() => catalog.value?.assets.find((asset) => asset.id === selectedId.value) ?? null);
  const detailVisible = computed(() => selectedAsset.value !== null);
  const editorVisible = ref(false);
  const editorAssetId = ref<string | null>(null);
  const editorDefinition = shallowRef<AgentCatalogDefinition | null>(null);
  const editorDraft = shallowRef<AgentCatalogSaveRequest | null>(null);
  const editorReturn = shallowRef<{ request: AgentCatalogSaveRequest; definition: AgentCatalogDefinition | null } | null>(null);
  const canReturnToEditor = computed(() => plans.visible.value && plans.fromEditor.value && Boolean(editorReturn.value));
  const editorCategory = ref<AgentAssetCategory>("skill");
  const editorLoading = ref(false);
  const editorSaving = ref(false);
  const editorError = ref("");
  const libraryBusy = ref<Record<string, boolean>>({});
  const libraryErrors = ref<Record<string, string>>({});
  let detailRequest = 0;
  let editorRequest = 0;
  let disposed = false;
  const deferredLibraryRefreshes = new Map<string, string | undefined>();

  function hasTransientSurface() { return editorVisible.value || plans.visible.value || management.selection.value || relations.visible.value; }
  async function refreshCatalog(workspace?: string) {
    const key = agentEnvironmentKey(workspace);
    store.invalidate(workspace);
    if (!disposed && key === agentEnvironmentKey(options.workspace.value) && hasTransientSurface()) {
      deferredLibraryRefreshes.set(key, workspace);
      return null;
    }
    deferredLibraryRefreshes.delete(key);
    return store.refresh(workspace, true);
  }
  function flushDeferredLibraryRefreshes() {
    for (const [key, workspace] of deferredLibraryRefreshes) {
      if (!disposed && key === agentEnvironmentKey(options.workspace.value) && hasTransientSurface()) continue;
      deferredLibraryRefreshes.delete(key);
      void refreshCatalog(workspace);
    }
  }
  function closeDetail() { detailRequest += 1; selectedId.value = null; selectedFeature.value = null; }
  function openDetail(id: string, feature?: string) {
    editorReturn.value = null;
    management.close(); relations.close(); plans.close();
    detailRequest += 1; selectedId.value = id; selectedFeature.value = feature ?? null;
  }
  function closeEditor() {
    editorRequest += 1; editorVisible.value = false; editorLoading.value = false; editorSaving.value = false;
    editorDefinition.value = null; editorDraft.value = null; editorAssetId.value = null; editorError.value = "";
  }
  async function openEditor(id: string | null, category: AgentAssetCategory) {
    editorReturn.value = null;
    closeEditor(); management.close(); relations.close(); plans.close();
    const request = ++editorRequest;
    editorCategory.value = category; editorAssetId.value = id; editorVisible.value = true;
    if (!id) return;
    editorLoading.value = true;
    try {
      const definition = await store.definition(id);
      if (definition.assetId !== id) throw new Error("共享定义返回的资源不一致");
      if (!disposed && request === editorRequest) editorDefinition.value = definition;
    } catch (failure) { if (!disposed && request === editorRequest) editorError.value = agentEnvironmentErrorMessage(failure); }
    finally { if (request === editorRequest) editorLoading.value = false; }
  }
  function retryEditor() {
    if (!editorVisible.value || editorLoading.value || editorSaving.value || !editorAssetId.value) return;
    return openEditor(editorAssetId.value, editorCategory.value);
  }
  function validEditorRequest(request: AgentCatalogSaveRequest) {
    if (!editorVisible.value || editorLoading.value || editorSaving.value) return false;
    const definition = editorDefinition.value;
    if (editorAssetId.value && definition?.assetId !== editorAssetId.value) return false;
    if (request.assetId !== editorAssetId.value || request.expectedVersion !== (definition?.version ?? null)) {
      editorError.value = "编辑目标已变化，请重新打开共享定义";
      return false;
    }
    return true;
  }
  async function saveDefinition(request: AgentCatalogSaveRequest) {
    if (!validEditorRequest(request)) return;
    const current = editorRequest;
    const workspace = options.workspace.value;
    editorSaving.value = true; editorError.value = "";
    try {
      const definition = await store.save(request);
      if (!disposed && current === editorRequest) {
        closeEditor(); selectedId.value = definition.assetId;
        Message.success(definition.version === request.expectedVersion
          ? `内容未变化，保留共享库 v${definition.version}` : "已保存到共享库，可从顶部“共享库”查看；尚未应用到其他 Agent");
      }
      await refreshCatalog(workspace);
      return definition;
    } catch (failure) { if (!disposed && current === editorRequest) editorError.value = agentEnvironmentErrorMessage(failure); }
    finally { if (current === editorRequest) editorSaving.value = false; }
  }
  function saveAndApply(request: AgentCatalogSaveRequest) {
    if (!validEditorRequest(request)) return;
    const draft = JSON.parse(JSON.stringify(request)) as AgentCatalogSaveRequest;
    editorReturn.value = { request: draft, definition: editorDefinition.value };
    closeEditor(); closeDetail(); management.close(); relations.close();
    return plans.open({ source: { kind: "draft", definition: draft }, action: "applyDefinition", name: draft.name });
  }
  function closePlan() {
    const saved = canReturnToEditor.value ? editorReturn.value : null;
    plans.close(); editorReturn.value = null;
    if (!saved || disposed) return;
    // Restore the submitted draft in the editor; preview/cancel does not save a shared version.
    editorRequest += 1;
    editorDefinition.value = saved.definition;
    editorAssetId.value = saved.request.assetId;
    editorCategory.value = saved.request.category;
    editorDraft.value = saved.request;
    editorLoading.value = false; editorSaving.value = false; editorError.value = "";
    editorVisible.value = true;
  }
  function confirmPlan() {
    if (!plans.canConfirm.value) return;
    editorReturn.value = null;
    plans.confirm();
  }
  async function adoptBinding(bindingId: string) {
    const original = catalog.value?.assets.flatMap((asset) => asset.bindings).find((binding) => binding.id === bindingId);
    if (!original || libraryBusy.value[bindingId]) return;
    const navigation = options.navigationRevision.value;
    const detail = detailRequest;
    const editor = editorRequest;
    const plan = plans.revision.value;
    const workspace = options.workspace.value;
    const isCurrent = () => !disposed && navigation === options.navigationRevision.value && detail === detailRequest
      && editor === editorRequest && plan === plans.revision.value && workspace === options.workspace.value;
    libraryBusy.value[bindingId] = true; delete libraryErrors.value[bindingId];
    try {
      const snapshot = await store.ensureReady(workspace);
      if (!isCurrent()) return;
      const binding = snapshot.assets.flatMap((asset) => asset.bindings).find((binding) => binding.id === bindingId);
      if (!binding || binding.native.revision.identity !== original.native.revision.identity) {
        throw new Error("来源配置已变化，请核对当前内容后重新收录");
      }
      const definition = await store.adopt({ bindingId, expectedRevision: snapshot.revision, workspace: workspace ?? null });
      const refreshed = await refreshCatalog(workspace);
      if (refreshed && !disposed && navigation === options.navigationRevision.value && detail === detailRequest && !hasTransientSurface()) {
        selectedId.value = definition.assetId;
        Message.success("已保存到共享库，可从顶部“共享库”查看");
      }
    } catch (failure) {
      if (!disposed && navigation === options.navigationRevision.value && detail === detailRequest && editor === editorRequest && plan === plans.revision.value) {
        libraryErrors.value[bindingId] = agentEnvironmentErrorMessage(failure);
      }
    } finally { libraryBusy.value[bindingId] = false; }
  }
  async function deleteDefinition(id: string) {
    const asset = catalog.value?.assets.find((item) => item.id === id);
    if (!asset?.definitionRemoval.available || asset.version === null || libraryBusy.value[id]) return;
    const navigation = options.navigationRevision.value;
    const detail = detailRequest;
    const editor = editorRequest;
    const plan = plans.revision.value;
    const workspace = options.workspace.value;
    const isCurrent = () => !disposed && navigation === options.navigationRevision.value && detail === detailRequest
      && editor === editorRequest && plan === plans.revision.value && workspace === options.workspace.value;
    libraryBusy.value[id] = true; delete libraryErrors.value[id];
    try {
      const snapshot = await store.ensureReady(workspace);
      if (!isCurrent()) return;
      const latest = snapshot.assets.find((item) => item.id === id);
      if (!latest || latest.version !== asset.version) throw new Error("共享定义已变化，请核对当前版本后重新删除");
      if (!latest.definitionRemoval.available) throw new Error(latest.definitionRemoval.reason || "当前共享定义无法删除，请核对使用状态");
      await store.deleteDefinition({ assetId: id, expectedVersion: asset.version, expectedRevision: snapshot.revision, workspace: workspace ?? null });
      if (!disposed && navigation === options.navigationRevision.value && detail === detailRequest && selectedId.value === id && !hasTransientSurface()) {
        closeDetail();
        Message.success("已从共享库删除");
      }
      await refreshCatalog(workspace);
    } catch (failure) {
      if (!disposed && navigation === options.navigationRevision.value && detail === detailRequest && editor === editorRequest && plan === plans.revision.value) {
        libraryErrors.value[id] = agentEnvironmentErrorMessage(failure);
      }
    } finally { libraryBusy.value[id] = false; }
  }
  function openPlan(id: string, action: AgentCatalogAction, targets: string[] = []) {
    const asset = catalog.value?.assets.find((item) => item.id === id);
    if (!asset) return;
    editorReturn.value = null;
    management.close(); relations.close();
    if (action === "applyDefinition" && !asset.application.available) { openDetail(id); return; }
    const source: AgentCatalogPlanSource = action === "applyDefinition" && asset.application.sourceBindingId
      ? { kind: "nativeBinding", assetId: id, bindingId: asset.application.sourceBindingId }
      : { kind: "catalog", assetId: id, expectedVersion: asset.version };
    return plans.open({ source, action, name: asset.name, targets });
  }
  function openAgentPanel(id: string, kind: AgentCliKind | null) {
    editorReturn.value = null;
    plans.close(); closeEditor(); relations.close();
    if (!kind) return management.close();
    return management.open(id, kind);
  }
  function invalidate() { editorReturn.value = null; closeDetail(); closeEditor(); plans.close(); management.close(); relations.close(); }
  watch(options.navigationRevision, invalidate, { flush: "sync" });
  watch([editorVisible, plans.visible, management.selection, relations.visible, options.workspace], flushDeferredLibraryRefreshes, { flush: "post" });
  // Catalog publication is metadata refresh, not navigation. Preserve selected
  // resources and drafts; each surface invalidates its own preview token.
  watch(catalog, () => {
    if (selectedId.value && !selectedAsset.value) closeDetail();
  }, { flush: "sync" });
  onScopeDispose(() => { disposed = true; invalidate(); flushDeferredLibraryRefreshes(); });

  return { catalog, refresh: refreshCatalog, selectedId, selectedFeature, selectedAsset, detailVisible, openDetail, closeDetail,
    editorVisible, editorAssetId, editorDefinition, editorDraft, editorCategory, editorLoading, editorSaving, editorError, closeEditor, openEditor, retryEditor, saveDefinition, saveAndApply,
    libraryBusy, libraryErrors, adoptBinding, deleteDefinition, management, openAgentPanel, relations,
    planVisible: plans.visible, planName: plans.name, planAction: plans.action,
    selectedTargets: plans.selected, pendingPlan: plans.preview, preparing: plans.preparing, planError: plans.error, planExpired: plans.expired, canConfirm: plans.canConfirm, targetChoices: plans.choices,
    configurationSelection: plans.configurationSelection, comparisonAssetId: plans.comparisonAssetId,
    openPlan, closePlan, canReturnToEditor, setTargets: plans.setTargets, preparePlan: plans.prepare, confirmPlan, invalidate };
}
