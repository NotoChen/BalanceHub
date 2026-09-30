import { computed, onScopeDispose, ref, shallowRef } from "vue";
import { discardAgentConfigurationEdit, planAgentConfigurationSave } from "../api/agent-configuration";
import { useAgentConfigurationStore, type AgentConfigurationTaskContext } from "../stores/agent-configuration";
import { useAgentConfigurationSourcesStore, type AgentConfigurationPublisher } from "../stores/agent-configuration-sources";
import type {
  AgentConfigurationDiagnostic, AgentConfigurationEdit, AgentConfigurationPlan, AgentConfigurationSaveRequest,
} from "../stores/agent-configuration-types";
import { agentConfigurationErrorDiagnostics, agentConfigurationErrorMessage } from "../utils/agent-configuration-display";
import { withTimeout } from "../utils/promise-timeout";

import type { AgentMcpEditorDraft } from "./useAgentMcpEditor";

type EditLoader = (isCurrent: () => boolean, publish: AgentConfigurationPublisher, signal: AbortSignal) => Promise<AgentConfigurationEdit>;
interface PendingEdit { loader: EditLoader; context: AgentConfigurationTaskContext; key: string }

/** Shared by native files and Provider candidates. Never initialize an edit from a truncated read preview. */
export function useAgentConfigurationEditor() {
  const tasks = useAgentConfigurationStore();
  const sources = useAgentConfigurationSourcesStore();
  const visible = ref(false);
  const loading = ref(false);
  const preparing = ref(false);
  const edit = shallowRef<AgentConfigurationEdit | null>(null);
  const textDrafts = ref<Record<string, string>>({});
  const mcpDrafts = ref<Record<string, AgentMcpEditorDraft>>({});
  const plan = shallowRef<AgentConfigurationPlan | null>(null);
  const planVisible = ref(false);
  const error = ref("");
  const errorDiagnostics = ref<AgentConfigurationDiagnostic[]>([]);
  const title = ref("原生配置");
  const context = shallowRef<AgentConfigurationTaskContext | null>(null);
  const pendingEdit = shallowRef<PendingEdit | null>(null);
  const currentLoader = shallowRef<EditLoader | null>(null);
  const now = ref(Date.now());
  let requestId = 0;
  let draftRevision = 0;
  let planRequest = 0;
  let disposed = false;
  let draftKey = "";
  let expiryTimer: ReturnType<typeof globalThis.setInterval> | null = null;
  let releaseSources: (() => void) | null = null;
  let loadController: AbortController | null = null;
  function releaseSourceHold() { releaseSources?.(); releaseSources = null; }
  const submitted = computed(() => Boolean(edit.value && tasks.ownsEdit(edit.value.editId)));
  const hasDraft = computed(() => {
    const current = edit.value;
    if (!current || tasks.operationFor(current.editId)?.outcome === "appliedVerified") return false;
    return current.documents.some((document) => document.creating || mcpDrafts.value[document.sourceId]?.modified || textDrafts.value[document.sourceId] !== document.originalText);
  });
  const pendingLabel = computed(() => pendingEdit.value?.context.label ?? "");
  const submissionError = computed(() => edit.value ? tasks.startErrors[edit.value.editId] ?? tasks.operationFor(edit.value.editId)?.message ?? "" : "");
  const submissionDiagnostics = computed(() => edit.value ? tasks.startDiagnostics[edit.value.editId] ?? [] : []);
  const canReload = computed(() => {
    const current = edit.value;
    if (!currentLoader.value || loading.value) return false;
    if (!submitted.value) return true;
    if (!current) return false;
    return Boolean(tasks.declined[current.editId] || tasks.operationFor(current.editId)?.phase === "completed");
  });
  const editExpired = computed(() => Boolean(edit.value && (!Number.isFinite(Date.parse(edit.value.expiresAt)) || Date.parse(edit.value.expiresAt) <= now.value)));
  const planExpired = computed(() => Boolean(plan.value && (!Number.isFinite(Date.parse(plan.value.expiresAt)) || Date.parse(plan.value.expiresAt) <= now.value)));
  const canPrepare = computed(() => visible.value && hasDraft.value && Boolean(edit.value?.documents.some((document) => !document.readOnlyReason)) && !submitted.value && !pendingEdit.value && !loading.value && !preparing.value && !editExpired.value);
  const canConfirm = computed(() => visible.value && planVisible.value && Boolean(plan.value) && !submitted.value && !pendingEdit.value && !planExpired.value && !editExpired.value && !loading.value && !preparing.value);

  function startClock() {
    now.value = Date.now();
    if (expiryTimer === null) expiryTimer = globalThis.setInterval(() => { now.value = Date.now(); }, 1000);
  }
  function stopClock() { if (expiryTimer !== null) globalThis.clearInterval(expiryTimer); expiryTimer = null; }
  function discard(editId: string) {
    if (!tasks.ownsEdit(editId)) void withTimeout(discardAgentConfigurationEdit(editId), 10_000, "关闭编辑超时").catch(() => {});
  }
  function invalidatePlan() {
    draftRevision += 1;
    planRequest += 1;
    plan.value = null;
    planVisible.value = false;
    preparing.value = false;
    error.value = "";
    errorDiagnostics.value = [];
  }
  function clearEdit() {
    if (edit.value) discard(edit.value.editId);
    edit.value = null;
    textDrafts.value = {};
    mcpDrafts.value = {};
    error.value = "";
    errorDiagnostics.value = [];
    currentLoader.value = null;
    context.value = null;
    draftKey = "";
  }
  function close() {
    requestId += 1;
    loadController?.abort(); loadController = null;
    planRequest += 1;
    visible.value = false;
    loading.value = false;
    preparing.value = false;
    planVisible.value = false;
    plan.value = null;
    pendingEdit.value = null;
    stopClock();
    releaseSourceHold();
    // Keep unconfirmed changes in memory, including failed or pending saves, with their original revision.
    if (!hasDraft.value) clearEdit();
  }

  async function load(loader: EditLoader, target: AgentConfigurationTaskContext, key: string) {
    if (disposed) return false;
    loadController?.abort();
    const controller = new AbortController();
    loadController = controller;
    const currentRequest = ++requestId;
    invalidatePlan();
    context.value = { ...target };
    draftKey = key;
    releaseSources ??= sources.hold(target.agentKind);
    currentLoader.value = loader;
    title.value = target.label;
    visible.value = true;
    loading.value = true;
    let expiredRequest = false;
    const isCurrent = () => !disposed && !expiredRequest && currentRequest === requestId;
    const pending = Promise.resolve().then(() => {
      if (!isCurrent()) throw new Error("编辑读取已取消");
      return sources.transaction(target.agentKind, (publish) => {
        if (!isCurrent()) throw new Error("编辑读取已取消");
        return loader(isCurrent, publish, controller.signal).then((result) => {
          if (!isCurrent()) discard(result.editId);
          return result;
        });
      }, 60_000, controller.signal);
    });
    try {
      const result = await withTimeout(pending, 30_000, "读取配置文件超时");
      if (!isCurrent()) return false;
      if (result.agentKind !== target.agentKind) { discard(result.editId); throw new Error("编辑目标不一致"); }
      const previousEditId = edit.value?.editId;
      edit.value = result;
      mcpDrafts.value = {};
      textDrafts.value = Object.fromEntries(result.documents.map((document) => [document.sourceId, document.text]));
      draftRevision += 1;
      startClock();
      if (previousEditId && previousEditId !== result.editId) discard(previousEditId);
      return true;
    } catch (failure) {
      expiredRequest = true;
      controller.abort();
      if (!disposed && currentRequest === requestId) {
        error.value = agentConfigurationErrorMessage(failure, edit.value ? "重新读取失败或超时，原草稿仍保留；请重试" : "无法读取配置编辑内容，请重试");
        errorDiagnostics.value = agentConfigurationErrorDiagnostics(failure);
      }
      return false;
    } finally {
      if (currentRequest === requestId) loading.value = false;
    }
  }
  function reopenDraft() {
    if (!edit.value || !context.value || disposed) return;
    releaseSources ??= sources.hold(context.value.agentKind);
    visible.value = true;
    planVisible.value = false;
    startClock();
  }
  function resumeDraft(key: string) {
    if (!hasDraft.value || draftKey !== key) return false;
    pendingEdit.value = null;
    reopenDraft();
    return true;
  }
  function open(loader: EditLoader, target: AgentConfigurationTaskContext, key: string) {
    if (disposed) return Promise.resolve(false);
    if (resumeDraft(key)) return Promise.resolve(true);
    if (hasDraft.value) {
      pendingEdit.value = { loader, context: { ...target }, key };
      reopenDraft();
      return Promise.resolve(false);
    }
    close();
    return load(loader, target, key);
  }
  function keepDraft() { pendingEdit.value = null; }
  function discardDraft() { close(); clearEdit(); }
  function openPending() {
    const pending = pendingEdit.value;
    if (!pending) return;
    discardDraft();
    return load(pending.loader, pending.context, pending.key);
  }

  function setText(sourceId: string, text: string) {
    if (loading.value || submitted.value || !edit.value?.documents.some((document) => document.sourceId === sourceId && !document.readOnlyReason)) return;
    if (textDrafts.value[sourceId] === text) return;
    textDrafts.value[sourceId] = text;
    invalidatePlan();
  }
  function setMcpDraft(sourceId: string, draft: AgentMcpEditorDraft) {
    if (loading.value || submitted.value || !edit.value?.documents.some((document) => document.sourceId === sourceId && !document.readOnlyReason)) return;
    const previous = mcpDrafts.value[sourceId];
    if (previous && JSON.stringify(previous) === JSON.stringify(draft)) return;
    mcpDrafts.value[sourceId] = draft;
    if (draft.modified || previous?.modified) invalidatePlan();
  }
  async function prepare() {
    const current = edit.value;
    if (!current || !canPrepare.value || Object.values(mcpDrafts.value).some((draft) => draft.validation)) return;
    const request = ++planRequest;
    const generation = requestId;
    const revision = draftRevision;
    const input: AgentConfigurationSaveRequest = {
      editId: current.editId, expectedRevision: current.revision,
      documents: current.documents.map((document) => ({ sourceId: document.sourceId, text: textDrafts.value[document.sourceId] })),
    };
    preparing.value = true;
    error.value = "";
    errorDiagnostics.value = [];
    plan.value = null;
    try {
      const pending = planAgentConfigurationSave(input);
      const result = await withTimeout(pending, 30_000, "预览更改超时");
      if (disposed || generation !== requestId || request !== planRequest || revision !== draftRevision) return;
      if (result.editId !== current.editId) throw new Error("计划目标不一致");
      plan.value = result;
      planVisible.value = true;
      now.value = Date.now();
    } catch (failure) {
      if (!disposed && generation === requestId && request === planRequest) {
        error.value = agentConfigurationErrorMessage(failure, "无法预览更改，草稿已保留，请重试");
        errorDiagnostics.value = agentConfigurationErrorDiagnostics(failure);
      }
    } finally {
      if (generation === requestId && request === planRequest) preparing.value = false;
    }
  }

  function backToDraft() { planVisible.value = false; }
  function confirm() {
    const current = plan.value;
    if (!current || !context.value || !canConfirm.value) return;
    if (!tasks.reserve({ editId: current.editId, planToken: current.token }, context.value)) return;
    // Do not call close(): the store owns the consumed edit and its plan now.
    requestId += 1;
    planRequest += 1;
    visible.value = false;
    planVisible.value = false;
    preparing.value = false;
    stopClock();
    releaseSourceHold();
    void tasks.submit(current.editId);
  }
  function reload() { if (canReload.value && currentLoader.value && context.value && !pendingEdit.value) return load(currentLoader.value, context.value, draftKey); }

  onScopeDispose(() => { disposed = true; discardDraft(); });
  return { visible, loading, preparing, edit, textDrafts, mcpDrafts, plan, planVisible, error, errorDiagnostics, title,
    context, hasDraft, pendingLabel, submitted, submissionError, submissionDiagnostics, canReload, editExpired, planExpired, canPrepare, canConfirm,
    open, close, setText, setMcpDraft, prepare, backToDraft, confirm, resumeDraft, keepDraft, discardDraft, openPending, reload };
}
