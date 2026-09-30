import { computed, reactive, ref, shallowRef, watch, type Ref, type UnwrapNestedRefs } from "vue";
import { beginAgentResourceEdit } from "../api/agent-configuration";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentCatalogAsset, AgentCatalogBinding, AgentCatalogSaveRequest, AgentCatalogContentSource, AgentCatalogContentFile } from "../stores/agent-catalog-types";
import type { useAgentCatalogConsole } from "./useAgentCatalogConsole";
import { useAgentConfigurationEditor } from "./useAgentConfigurationEditor";
import { useAgentCatalogAgentPanel } from "./useAgentCatalogAgentPanel";
import { useAgentCatalogContent } from "./useAgentCatalogContent";

export function useAgentResourceContent(options: {
  catalog: UnwrapNestedRefs<ReturnType<typeof useAgentCatalogConsole>>;
  workspace: Ref<string | undefined>;
}) {
  const catalog = options.catalog;
  const store = useAgentCatalogStore();
  const editor = reactive(useAgentConfigurationEditor());
  const bindingId = ref<string | null>(null);
  const view = ref<"native" | "read" | "shared">("read");
  const sharedView = computed(() => view.value === "shared");
  const retainedSelection = shallowRef<{ asset: AgentCatalogAsset; binding: AgentCatalogBinding } | null>(null);
  let viewRevision = 0;
  let requestedKey = "";
  const nativeSelection = computed(() => {
    const nativeId = editor.edit?.resource?.assetId;
    if (!nativeId) return null;
    for (const asset of catalog.catalog?.assets ?? []) {
      const binding = asset.bindings.find((item) => item.native.stableId === nativeId);
      if (binding) return { asset, binding };
    }
    return retainedSelection.value?.binding.native.stableId === nativeId ? retainedSelection.value : null;
  });
  // While a draft decision is pending, the heading and actions still belong to
  // the content being displayed, rather than the resource requested next.
  const showingNative = computed(() => view.value === "native" && editor.visible && !editor.loading);
  const asset = computed(() => showingNative.value
    ? nativeSelection.value?.asset ?? catalog.selectedAsset : catalog.selectedAsset);
  const binding = computed(() => showingNative.value && nativeSelection.value
    ? nativeSelection.value.binding
    : asset.value?.bindings.find((item) => item.id === bindingId.value) ?? null);
  const management = reactive(useAgentCatalogAgentPanel({ catalog: computed(() => catalog.catalog), workspace: options.workspace }));
  const managementExpanded = ref(false);
  watch([() => catalog.detailVisible, managementExpanded, () => asset.value?.id, options.workspace], () => {
    if (!catalog.detailVisible || !managementExpanded.value || !asset.value) { management.close(); return; }
    void management.open(asset.value.id, null);
  });
  const reading = computed(() => view.value === "read" && Boolean(asset.value));
  const reader = reactive(useAgentCatalogContent({
    assetId: computed(() => reading.value ? asset.value?.id ?? null : null),
    workspace: options.workspace,
    contentRevision: computed(() => asset.value?.contentRevision),
  }));
  const draftBindingId = computed(() => editor.hasDraft && nativeSelection.value?.asset.id === asset.value?.id ? nativeSelection.value?.binding.id ?? null : null);
  const relatedBindings = computed(() => reading.value ? asset.value?.bindings ?? [] : binding.value ? [binding.value] : []);
  function relatedResources(ids: Set<string>) {
    const rows = new Map<string, { stableId: string; label: string; category: AgentCatalogAsset["category"] }>();
    for (const native of catalog.catalog?.inventory.assets ?? []) {
      if (!ids.has(native.stableId)) continue;
      const row = catalog.catalog?.assets.find((item) => item.bindings.some((binding) => binding.native.stableId === native.stableId));
      rows.set(row?.id ?? native.stableId, { stableId: native.stableId, label: row?.name ?? native.label, category: native.category });
    }
    return [...rows.values()].sort((left, right) => left.label.localeCompare(right.label));
  }
  const parents = computed(() => relatedResources(new Set(relatedBindings.value.flatMap((binding) => binding.native.relationships.providedBy ? [binding.native.relationships.providedBy] : []))));
  const children = computed(() => {
    const parentIds = new Set(relatedBindings.value.map((binding) => binding.native.stableId));
    return relatedResources(new Set((catalog.catalog?.inventory.assets ?? []).filter((item) => item.relationships.providedBy && parentIds.has(item.relationships.providedBy)).map((item) => item.stableId)));
  });
  const editorDocumentId = ref<string | null>(null);
  const sharedAgentKind = ref<AgentCliKind | null>(null);
  const definitionVisible = computed(() => catalog.editorVisible && catalog.editorAssetId === asset.value?.id);

  function readBinding(target: AgentCatalogAsset, selected: AgentCatalogBinding, documentId: string | null = null) {
    const workspace = options.workspace.value ?? null;
    const assetId = selected.native.stableId;
    const key = JSON.stringify(["resource", workspace, assetId]);
    const sameEdit = editor.edit?.resource?.assetId === assetId && editor.context?.workspace === workspace;
    const hasDocument = !documentId || Boolean(editor.edit?.documents.some((document) => document.sourceId === documentId));
    if (sameEdit && editor.hasDraft && !hasDocument) {
      reader.error = "现有草稿不包含所选文件，请先继续编辑或放弃草稿，再重新读取内容。";
      return Promise.resolve(false);
    }
    if (sameEdit && hasDocument && documentId) editorDocumentId.value = documentId;
    if (sameEdit && hasDocument && editor.visible && !editor.loading && view.value === "native" && !editor.pendingLabel && requestedKey === key) return Promise.resolve(true);
    viewRevision += 1;
    requestedKey = key;
    view.value = "native";
    bindingId.value = selected.id;
    return editor.open(async (isCurrent, _publish, signal) => {
      const ready = await store.ensureReady(workspace ?? undefined);
      if (!isCurrent() || signal.aborted || workspace !== (options.workspace.value ?? null)) throw new Error("编辑读取已取消");
      if (!ready.assets.some((asset) => asset.bindings.some((binding) => binding.native.stableId === assetId))) {
        throw new Error("来源配置已移除，请重新选择资源");
      }
      const result = await beginAgentResourceEdit({ assetId, workspace, documentId }, signal);
      if (isCurrent()) editorDocumentId.value = documentId;
      return result;
    },
      { agentKind: selected.native.agentKind, workspace, label: target.name }, key);
  }
  function selectBinding(id: string, documentId?: string | null) {
    const current = asset.value;
    const selected = current?.bindings.find((item) => item.id === id);
    if (!current || !selected || editor.pendingLabel) return;
    void readBinding(current, selected, documentId ?? null);
  }
  function showShared() {
    const current = asset.value;
    if (!current || current.ownership !== "managed" || editor.pendingLabel) return;
    viewRevision += 1;
    view.value = "shared";
    editor.close();
    if (!catalog.editorVisible || catalog.editorAssetId !== current.id) void catalog.openEditor(current.id, current.category);
  }
  function showContent() {
    const current = asset.value;
    if (current) readContent(current);
  }
  function readContent(current: AgentCatalogAsset) {
    viewRevision += 1;
    view.value = "read";
    bindingId.value = null;
    editor.close();
    if (catalog.selectedId !== current.id) catalog.openDetail(current.id);
  }
  function editContent(source: AgentCatalogContentSource, file: AgentCatalogContentFile) {
    if (file.readOnlyReason) return;
    if (source.bindingId === null) { sharedAgentKind.value = source.agentKind; showShared(); }
    else if (!asset.value?.bindings.some((item) => item.id === source.bindingId)) {
      reader.error = "此文件的使用范围已变化，请刷新资源列表后编辑";
    } else selectBinding(source.bindingId, file.documentId);
  }
  function close() {
    viewRevision += 1;
    catalog.closeDetail();
    catalog.closeEditor();
    view.value = "native";
    bindingId.value = null;
    editor.close();
  }
  async function saveShared(request: AgentCatalogSaveRequest) {
    const currentId = asset.value?.id;
    const revision = viewRevision;
    const saved = await catalog.saveDefinition(request);
    if (saved && revision === viewRevision && catalog.selectedId === currentId && sharedView.value) {
      await catalog.openEditor(saved.assetId, saved.category);
    }
  }
  function reloadShared() {
    const current = asset.value;
    if (current && sharedView.value && !catalog.editorSaving) return catalog.openEditor(current.id, current.category);
  }
  function applyShared(request: AgentCatalogSaveRequest) {
    const result = catalog.saveAndApply(request);
    if (catalog.planVisible) close();
    return result;
  }

  watch([() => catalog.selectedAsset?.id, () => catalog.selectedFeature?.startsWith("binding:") ? catalog.selectedFeature : null, options.workspace], () => {
    const current = catalog.selectedAsset;
    if (!current) {
      if (catalog.editorAssetId) catalog.closeEditor();
      view.value = "native"; bindingId.value = null; editor.close();
      return;
    }
    if (catalog.editorAssetId && catalog.editorAssetId !== current.id) catalog.closeEditor();
    const focusedId = catalog.selectedFeature?.startsWith("binding:") ? catalog.selectedFeature.slice(8) : null;
    // Reading is asset-scoped. A binding is selected only by an explicit edit
    // or when the native editor restores an existing draft after navigation.
    const selectedBindingId = nativeSelection.value?.binding.id;
    const restoringDraft = view.value === "native" && editor.visible && !editor.loading
      && editor.context?.workspace === (options.workspace.value ?? null)
      && nativeSelection.value?.asset.id === current.id && selectedBindingId === focusedId;
    if (!restoringDraft) { readContent(current); return; }
    const selected = current.bindings.find((item) => item.id === focusedId);
    if (selected) void readBinding(current, selected);
  }, { immediate: true });

  watch(() => editor.edit?.editId, () => {
    if (nativeSelection.value) retainedSelection.value = nativeSelection.value;
  });

  watch(() => [editor.pendingLabel, editor.loading, editor.edit?.resource?.assetId] as const, () => {
    const selected = nativeSelection.value;
    if (!selected || !editor.visible || editor.loading || editor.pendingLabel || view.value !== "native") return;
    bindingId.value = selected.binding.id;
    requestedKey = JSON.stringify(["resource", editor.context?.workspace ?? null, selected.binding.native.stableId]);
    // "Continue this draft" also restores its resource selection. Request IDs
    // in the shared editor reject late content from the abandoned navigation.
    if (catalog.selectedId !== selected.asset.id && catalog.catalog?.assets.some((asset) => asset.id === selected.asset.id)) {
      catalog.openDetail(selected.asset.id, `binding:${selected.binding.id}`);
    }
  });
  watch(() => editor.visible, (visible, previous) => {
    if (!visible && previous && view.value === "native" && binding.value && catalog.detailVisible) close();
  });

  return { editor, asset, binding, sharedView, reading, reader, draftBindingId, definitionVisible, parents, children,
    management, managementExpanded, editorDocumentId, sharedAgentKind, selectBinding, showShared, showContent, editContent, close, saveShared, reloadShared, applyShared };
}
