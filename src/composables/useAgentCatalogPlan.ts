import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentAssetCatalog, AgentCatalogAction, AgentCatalogConfigurationSelection, AgentCatalogPlan, AgentCatalogPlanSource, AgentCatalogTargetPlan } from "../stores/agent-catalog-types";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { agentEnvironmentErrorMessage } from "../stores/agent-environment";
import { agentAssetScopeLabels } from "./useAgentAssetCatalog";
import { cancelAgentCatalogRead } from "../api/agent-catalog";

export interface AgentCatalogTargetChoice {
  id: string; label: string; agentKind: AgentCliKind; detail: string; available: boolean; reason: string;
}

export function useAgentCatalogPlan(options: {
  catalog: Ref<AgentAssetCatalog | null>; workspace: Ref<string | undefined>;
  afterMutation: (workspace?: string) => Promise<unknown>;
}) {
  const store = useAgentCatalogStore();
  const visible = ref(false);
  const name = ref("");
  const action = ref<AgentCatalogAction>("applyDefinition");
  const source = shallowRef<AgentCatalogPlanSource | null>(null);
  const fromEditor = computed(() => source.value?.kind === "draft");
  const comparisonAssetId = computed(() => source.value && source.value.kind !== "draft" ? source.value.assetId : null);
  const selected = ref<string[]>([]);
  const optionsFromBackend = shallowRef<AgentCatalogTargetPlan[]>([]);
  const configurationOptions = shallowRef<AgentCatalogConfigurationSelection | null>(null);
  const explicitTargets = shallowRef<string[]>([]);
  const configurationSelection = computed(() => explicitTargets.value.length ? null : configurationOptions.value);
  const preview = shallowRef<AgentCatalogPlan | null>(null);
  const preparing = ref(false);
  const error = ref("");
  const revision = ref(0);
  const now = ref(Date.now());
  let catalogRevision = "";
  let planWorkspace: string | undefined;
  let request = 0;
  let disposed = false;
  let activeRead: string | null = null;
  function cancelRead() {
    if (activeRead) void cancelAgentCatalogRead(activeRead).catch(() => {});
    activeRead = null;
  }
  let timer: ReturnType<typeof globalThis.setInterval> | null = null;
  const expired = computed(() => Boolean(preview.value?.token && (!Number.isFinite(Date.parse(preview.value.expiresAt)) || Date.parse(preview.value.expiresAt) <= now.value)));
  const canConfirm = computed(() => Boolean(preview.value?.token && preview.value.planId && !preparing.value && !expired.value
    && selected.value.length && preview.value.targets.length === selected.value.length && preview.value.targets.every((target) => target.available && selected.value.includes(target.targetId))));
  const choices = computed<AgentCatalogTargetChoice[]>(() => optionsFromBackend.value
    .filter((target) => !explicitTargets.value.length || explicitTargets.value.includes(target.targetId))
    .map((target) => {
      const context = options.catalog.value?.inventory.contexts.find((item) => item.id === target.contextId);
      const configuration = configurationOptions.value?.choices.find((choice) => choice.targetIds.includes(target.targetId));
      const binding = options.catalog.value?.assets.flatMap((asset) => asset.bindings).find((binding) => binding.id === target.targetId);
      const multipleLocations = optionsFromBackend.value.filter((item) => item.agentKind === target.agentKind && item.scope === target.scope).length > 1;
      return { id: target.targetId, label: configuration?.label ?? target.label, agentKind: target.agentKind, available: target.available, reason: target.reason || "",
        detail: [agentAssetScopeLabels[target.scope], context?.profile !== "default" ? context?.profile : null, configuration?.detail, !configuration && multipleLocations ? binding?.native.path : null].filter(Boolean).join(" · ") };
    }));
  function clearPreview() {
    cancelRead();
    request += 1;
    revision.value += 1;
    preview.value = null;
    preparing.value = false;
    error.value = "";
    if (timer !== null) globalThis.clearInterval(timer);
    timer = null;
  }
  function close() {
    clearPreview(); visible.value = false; source.value = null;
    name.value = ""; selected.value = []; optionsFromBackend.value = [];
    configurationOptions.value = null; explicitTargets.value = [];
  }
  async function fetchPlan(targetIds: string[]) {
    const currentSource = source.value;
    if (!visible.value || !currentSource || disposed) return null;
    cancelRead();
    const requestId = crypto.randomUUID();
    activeRead = requestId;
    const current = ++request;
    revision.value += 1;
    const expectedAction = action.value;
    const workspace = planWorkspace;
    const isCurrent = () => !disposed && current === request && visible.value && workspace === options.workspace.value;
    preparing.value = true; error.value = ""; preview.value = null;
    try {
      const catalog = await store.ensureReady(workspace);
      if (!isCurrent()) return null;
      const changed = catalogRevision !== catalog.revision;
      let resolvedSource = currentSource;
      if (currentSource.kind !== "draft") {
        const asset = catalog.assets.find((asset) => asset.id === currentSource.assetId);
        if (!asset) throw new Error("此资源已不在当前目录，请返回列表选择现有资源");
        if (currentSource.kind === "catalog") resolvedSource = { ...currentSource, expectedVersion: asset.version };
        else if (!asset.bindings.some((binding) => binding.id === currentSource.bindingId)) throw new Error("来源配置已移除，请重新选择来源");
        name.value = asset.name;
      }
      source.value = resolvedSource;
      const requestFor = (ids: string[]) => ({ source: resolvedSource, action: expectedAction, targetIds: ids, expectedRevision: catalog.revision, workspace: workspace ?? null });
      if (targetIds.length && changed) {
        const availablePlan = await store.plan(requestFor([]), requestId);
        if (!isCurrent()) return null;
        if (catalog.revision !== options.catalog.value?.revision) throw new Error("目录在读取期间发生变化，请重新读取当前状态");
        optionsFromBackend.value = availablePlan.targets;
        configurationOptions.value = availablePlan.selection;
        catalogRevision = catalog.revision;
      }
      if (targetIds.length) {
        const allowed = configurationSelection.value
          ? configurationSelection.value.choices.filter((choice) => choice.available).flatMap((choice) => choice.targetIds)
          : choices.value.filter((choice) => choice.available).map((choice) => choice.id);
        if (targetIds.some((id) => !allowed.includes(id))) throw new Error("部分所选配置已变化，请调整选择后重新预览");
      }
      const result = await store.plan(requestFor(targetIds), requestId);
      if (!isCurrent()) return null;
      if (catalog.revision !== options.catalog.value?.revision) throw new Error("目录在读取期间发生变化，请重新读取当前状态");
      const expectedAsset = resolvedSource.kind === "draft" ? resolvedSource.definition.assetId : resolvedSource.assetId;
      if ((expectedAsset && result.assetId !== expectedAsset) || result.action !== expectedAction
        || (targetIds.length && (result.targets.length !== targetIds.length || result.targets.some((target) => !targetIds.includes(target.targetId))))) {
        throw new Error("返回的计划与所选资源或范围不一致，请重新读取");
      }
      // Candidate options are not a prepared preview, including when targets were preselected.
      preview.value = targetIds.length ? result : null;
      if (!targetIds.length) {
        optionsFromBackend.value = result.targets;
        // Explicit locations retain their destination; other actions keep their own chooser.
        configurationOptions.value = result.selection;
        catalogRevision = catalog.revision;
      }
      now.value = Date.now();
      if (timer !== null) globalThis.clearInterval(timer);
      timer = result.token ? globalThis.setInterval(() => { now.value = Date.now(); }, 1_000) : null;
      return result;
    } catch (failure) {
      if (!disposed && current === request) { cancelRead(); error.value = agentEnvironmentErrorMessage(failure); }
      return null;
    } finally { if (current === request) { preparing.value = false; activeRead = null; } }
  }
  async function open(input: { source: AgentCatalogPlanSource; action: AgentCatalogAction; name: string; targets?: string[] }) {
    close();
    if (!options.catalog.value) return;
    source.value = input.source; action.value = input.action; name.value = input.name;
    explicitTargets.value = [...(input.targets ?? [])];
    selected.value = [...explicitTargets.value];
    catalogRevision = "";
    planWorkspace = options.workspace.value;
    visible.value = true;
    const result = await fetchPlan([]);
    if (!result || !visible.value) return;
    const targets = input.targets ?? [];
    if (targets.some((id) => !choices.value.some((choice) => choice.id === id))) {
      error.value = "所选配置范围已变化，请重新选择";
      return;
    }
    selected.value = [...targets];
    if (targets.length) await fetchPlan([...targets]);
  }
  function setTargets(targets: string[]) {
    clearPreview();
    const allowed = configurationSelection.value
      ? configurationSelection.value.choices.filter((choice) => choice.available).flatMap((choice) => choice.targetIds)
      : choices.value.filter((choice) => choice.available).map((choice) => choice.id);
    selected.value = [...new Set(targets)].filter((id) => allowed.includes(id));
  }
  function prepare() { if (!preparing.value) return fetchPlan([...selected.value]); }
  function confirm() {
    now.value = Date.now();
    const result = preview.value;
    if (!result || !canConfirm.value) return;
    const workspace = planWorkspace;
    close();
    void store.apply(result, workspace, options.afterMutation);
  }
  watch(() => options.catalog.value?.revision, (next, previous) => {
    if (!visible.value || preparing.value || !previous || next === previous) return;
    clearPreview();
    error.value = "目录已更新，选择已保留，请重新预览当前配置";
  });
  onScopeDispose(() => { disposed = true; close(); });
  return { visible, name, action, fromEditor, comparisonAssetId, selected, preview, preparing, error, expired, canConfirm, choices, configurationSelection, revision, open, close, setTargets, prepare, confirm };
}
