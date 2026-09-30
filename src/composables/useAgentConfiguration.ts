import { computed, onScopeDispose, reactive, ref, shallowRef, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { beginAgentConfigurationEdit, openAgentConfigurationSource, readAgentConfigurationSource } from "../api/agent-configuration";
import { useAgentConfigurationSourcesStore } from "../stores/agent-configuration-sources";
import type { AgentConfigurationActionKind, AgentConfigurationReadResult, AgentConfigurationSnapshot, AgentConfigurationSource, AgentConfigurationSourceRequest } from "../stores/agent-configuration-types";
import type { AgentAssetAccessRisk, AgentCliKind } from "../stores/provider-types";
import { agentConfigurationErrorMessage, agentConfigurationFileName, agentConfigurationOpenAction } from "../utils/agent-configuration-display";
import { withTimeout } from "../utils/promise-timeout";
import { useAgentConfigurationEditor } from "./useAgentConfigurationEditor";
import type { AgentAssetAccessConfirmation } from "./useAgentAssetConsole";

export function useAgentConfiguration(options: { agentKind: Ref<AgentCliKind | null>; workspace: Ref<string | undefined> }) {
  const publications = useAgentConfigurationSourcesStore();
  const editor = reactive(useAgentConfigurationEditor());
  const entry = computed(() => options.agentKind.value ? publications.get(options.agentKind.value, options.workspace.value) : null);
  const snapshot = computed(() => entry.value?.value ?? null);
  const previewVisible = ref(false);
  const previewSource = shallowRef<AgentConfigurationSource | null>(null);
  const preview = shallowRef<AgentConfigurationReadResult | null>(null);
  const previewLoading = ref(false);
  const previewError = ref("");
  const opening = ref<Record<string, boolean>>({});
  const openErrors = ref<Record<string, string>>({});
  const accessConfirmation = shallowRef<AgentAssetAccessConfirmation | null>(null);
  let accessSource: AgentConfigurationSource | null = null;
  const sources = computed(() => snapshot.value?.sources ?? []);
  let generation = 0;
  let readRequest = 0;
  let openSequence = 0;
  const openRequests = new Map<string, number>();
  let disposed = false;
  let unsubscribe: (() => void) | null = null;
  let readController: AbortController | null = null;

  function sourceRequest(source: AgentConfigurationSource): AgentConfigurationSourceRequest | null {
    if (source.access.kind !== "ready") return null;
    return { sourceId: source.sourceId, accessId: source.access.accessId, environmentId: source.environmentId,
      workspace: source.workspace, expectedRevision: source.revision.identity };
  }
  function closePreview() {
    readRequest += 1;
    readController?.abort(); readController = null;
    previewVisible.value = false;
    previewLoading.value = false;
    previewSource.value = null;
    preview.value = null;
    previewError.value = "";
  }
  function closeAccessConfirmation() { accessConfirmation.value = null; accessSource = null; }
  function currentSource(result: AgentConfigurationSnapshot, sourceId: string, kind: AgentConfigurationActionKind) {
    const source = result.sources.find((item) => item.sourceId === sourceId);
    if (!source || source.access.kind !== "ready" || !source.actions.some((item) => item.action === kind && item.available)) {
      throw { kind: "sourceUnavailable", diagnostics: [] };
    }
    return source;
  }
  function invalidate() {
    generation += 1;
    closePreview();
    closeAccessConfirmation();
    editor.close();
    opening.value = {};
    openErrors.value = {};
    openRequests.clear();
  }
  async function read(source: AgentConfigurationSource) {
    closePreview();
    const currentRequest = ++readRequest;
    const controller = new AbortController();
    readController = controller;
    const currentGeneration = generation;
    previewVisible.value = true;
    previewSource.value = source;
    previewLoading.value = true;
    const isCurrent = () => !disposed && currentRequest === readRequest && currentGeneration === generation;
    try {
      await publications.transaction(source.agentKind, async (publish) => {
        if (!isCurrent()) return;
        const result = await publish(source.workspace);
        if (!isCurrent()) return;
        const fresh = currentSource(result, source.sourceId, "read");
        const request = sourceRequest(fresh);
        if (!request) return;
        previewSource.value = fresh;
        const content = await withTimeout(readAgentConfigurationSource(request), 20_000, "读取配置预览超时");
        if (!isCurrent()) return;
        if (content.sourceId !== fresh.sourceId || content.revision !== fresh.revision.identity) throw new Error("来源已变化");
        preview.value = content;
      }, 60_000, controller.signal);
    } catch (failure) {
      if (!disposed && currentRequest === readRequest && currentGeneration === generation) previewError.value = agentConfigurationErrorMessage(failure, "读取配置预览失败，请刷新来源");
    } finally { if (currentRequest === readRequest) previewLoading.value = false; }
  }
  async function executeOpen(source: AgentConfigurationSource, action: "open" | "reveal", risks: AgentAssetAccessRisk[]) {
    if (opening.value[source.sourceId]) return;
    const currentGeneration = generation;
    const currentRequest = ++openSequence;
    openRequests.set(source.sourceId, currentRequest);
    opening.value[source.sourceId] = true;
    delete openErrors.value[source.sourceId];
    const isCurrent = () => !disposed && currentGeneration === generation && openRequests.get(source.sourceId) === currentRequest;
    try {
      await publications.transaction(source.agentKind, async (publish) => {
        if (!isCurrent()) return;
        const result = await publish(source.workspace);
        if (!isCurrent()) return;
        const fresh = currentSource(result, source.sourceId, action);
        const capability = fresh.actions.find((item) => item.action === action);
        if (capability?.risks.some((risk) => !risks.includes(risk))) throw { kind: "accessExpired", diagnostics: [] };
        const request = sourceRequest(fresh);
        if (request) await withTimeout(openAgentConfigurationSource({ ...request, target: action === "reveal" ? "reveal" : "asset", acceptedRisks: risks }), 15_000, "系统打开超时");
      });
    } catch (failure) {
      if (!disposed && currentGeneration === generation && openRequests.get(source.sourceId) === currentRequest) {
        openErrors.value[source.sourceId] = agentConfigurationErrorMessage(failure, "无法打开此来源，请刷新后重试");
        Message.error(openErrors.value[source.sourceId]);
      }
    } finally {
      if (openRequests.get(source.sourceId) === currentRequest) { opening.value[source.sourceId] = false; openRequests.delete(source.sourceId); }
    }
  }
  function openFile(sourceId: string, displaySource?: AgentConfigurationSource) {
    const key = JSON.stringify(["native", options.agentKind.value, options.workspace.value ?? null, sourceId]);
    if (editor.resumeDraft(key)) return true;
    const source = displaySource ?? sources.value.find((item) => item.sourceId === sourceId);
    const kind = source && agentConfigurationOpenAction(source);
    if (!kind || source?.access.kind !== "ready") {
      Message.error("此配置文件当前不可访问，请刷新后重试");
      return false;
    }
    action(sourceId, kind, source);
    return true;
  }
  function editorSource(sourceId: string) {
    const target = editor.context;
    return target ? publications.get(target.agentKind, target.workspace ?? undefined).value?.sources.find((source) => source.sourceId === sourceId) ?? null : null;
  }
  function action(sourceId: string, kind: AgentConfigurationActionKind, displaySource?: AgentConfigurationSource) {
    const source = displaySource ?? sources.value.find((item) => item.sourceId === sourceId);
    const capability = source?.actions.find((item) => item.action === kind);
    if (!source || !capability?.available || source.access.kind !== "ready") return;
    if (kind === "read") { void read(source); return; }
    if (kind === "edit" || kind === "create") {
      const { sourceId: selectedSourceId, agentKind, workspace } = source;
      closePreview();
      void editor.open(async (isCurrent, publish) => {
        const currentGeneration = generation;
        // A publication replaces access references. Every read must obtain its own current authorization.
        const result = await publish(workspace);
        if (!isCurrent() || currentGeneration !== generation) throw new Error("来源读取已取消");
        if (result.agentKind !== agentKind) throw new Error("来源环境不一致");
        const freshSource = currentSource(result, selectedSourceId, kind);
        const request = freshSource && sourceRequest(freshSource);
        if (!request) throw { kind: "sourceUnavailable", diagnostics: [] };
        return beginAgentConfigurationEdit(request);
      }, { agentKind, workspace, label: agentConfigurationFileName(source) }, JSON.stringify(["native", agentKind, workspace, selectedSourceId]));
      return;
    }
    if (capability.risks.length) {
      accessSource = source;
      accessConfirmation.value = { kind: "source", id: sourceId, action: kind, accessId: source.access.accessId, environmentId: source.environmentId,
        workspaceKey: source.workspace ?? "", generation, label: source.label, path: source.path, risks: capability.risks };
    } else void executeOpen(source, kind, []);
  }
  function confirmAccess() {
    const confirmation = accessConfirmation.value;
    const source = accessSource;
    closeAccessConfirmation();
    if (!confirmation || confirmation.generation !== generation) return;
    if (source?.access.kind === "ready" && source.access.accessId === confirmation.accessId) void executeOpen(source, confirmation.action, confirmation.risks);
  }

  watch(() => [options.agentKind.value, options.workspace.value] as const, () => {
    unsubscribe?.(); unsubscribe = null;
    invalidate();
    if (options.agentKind.value) unsubscribe = publications.subscribe(options.agentKind.value, options.workspace.value);
  }, { immediate: true, flush: "sync" });
  onScopeDispose(() => { disposed = true; unsubscribe?.(); invalidate(); });
  return { editor, previewVisible, previewSource, preview, previewLoading, previewError,
    opening, accessConfirmation, openFile, editorSource, action, closePreview, closeAccessConfirmation, confirmAccess, invalidate };
}
