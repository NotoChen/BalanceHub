import { computed, onScopeDispose, ref, watch } from "vue";
import { readAgentMcpForm, renderAgentMcpForm } from "../api/agent-catalog";
import type { AgentMcpFormRead } from "../stores/agent-catalog-types";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentConfigurationFormat } from "../stores/agent-configuration-types";
import type { McpFieldDraft } from "../utils/mcp-form-fields";
import { withTimeout } from "../utils/promise-timeout";
import { useMcpConnectionDraft, type McpDraftState } from "./useMcpConnectionDraft";

/** In-memory only: includes incomplete fields that cannot yet produce valid native text. */
export interface AgentMcpEditorDraft {
  schema: AgentMcpFormRead; state: McpDraftState; baseline: string; sourceText: string;
  mode: "form" | "raw"; rawAtSwitch: string; rawChanged: boolean; pendingForm: boolean;
  modified: boolean; validation: string;
}
export interface AgentMcpEditorProps {
  modelValue: string; originalText: string; agentKind?: AgentCliKind | null;
  format?: AgentConfigurationFormat; readonly?: boolean; draft?: AgentMcpEditorDraft;
}
export function useAgentMcpEditor(props: Readonly<AgentMcpEditorProps>, emitText: (text: string) => void,
  emitValidation: (error: string) => void, emitDraft: (draft: AgentMcpEditorDraft) => void) {
  const connection = useMcpConnectionDraft();
  const mode = ref<"form" | "raw">("form");
  const loading = ref(false);
  const converting = ref(false);
  const error = ref("");
  const pendingForm = ref(false);
  const validation = ref("");
  let sourceText = "";
  let rawAtSwitch = "";
  const rawChanged = ref(false);
  let lastEmission: string | null = null;
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const firstError = computed(() => Object.values(connection.result.value.errors)[0] ?? "");
  const rawNotice = computed(() => mode.value === "raw" && pendingForm.value && !rawChanged.value
    ? "表单中有尚未同步的修改。这里显示最后一次有效原文；返回表单可继续填写，或选择以当前原文为准。" : "");
  function cancel() {
    generation += 1;
    if (timer) clearTimeout(timer);
    timer = null; converting.value = false; loading.value = false;
  }
  function validateWith(message: string) { validation.value = message; emitValidation(message); }
  function publishDraft() {
    if (!connection.schema.value || !connection.state.value) return;
    // Vue wraps nested values in proxies; serialize a detached UI snapshot, never structuredClone a proxy.
    emitDraft(JSON.parse(JSON.stringify({ schema: connection.schema.value, state: connection.state.value,
      baseline: connection.baseline.value, sourceText, mode: mode.value, rawAtSwitch, rawChanged: rawChanged.value,
      pendingForm: pendingForm.value, modified: connection.modified.value, validation: validation.value })) as AgentMcpEditorDraft);
  }
  function emit(text: string) { lastEmission = text; emitText(text); }
  async function read(text: string) {
    cancel();
    const current = generation;
    loading.value = true; error.value = ""; validateWith("正在读取连接字段…");
    try {
      const result = await withTimeout(readAgentMcpForm(text, props.format ?? "json", props.agentKind ?? null), 5_000, "读取连接字段超时，可继续编辑原文");
      if (current !== generation) return;
      connection.load(result); sourceText = text; mode.value = "form";
      pendingForm.value = false; rawChanged.value = false;
      validateWith(firstError.value); publishDraft();
    } catch (failure) {
      if (current !== generation) return;
      mode.value = "raw"; error.value = failure instanceof Error ? failure.message : String(failure);
      validateWith(rawNotice.value); publishDraft();
    } finally { if (current === generation) loading.value = false; }
  }
  function update() {
    if (props.readonly || loading.value || mode.value !== "form") return;
    cancel(); error.value = ""; pendingForm.value = true;
    const result = connection.result.value;
    if (!result.input || firstError.value) { validateWith(firstError.value); publishDraft(); return; }
    const current = generation;
    const kind = props.agentKind ?? null;
    converting.value = true; validateWith("正在整理配置草稿…"); publishDraft();
    timer = setTimeout(() => {
      timer = null;
      void withTimeout(renderAgentMcpForm(result.input!, sourceText, props.format ?? "json", kind), 5_000, "整理配置超时，表单内容已保留")
        .then((text) => { if (current === generation) { emit(text); pendingForm.value = false; validateWith(""); } })
        .catch((failure) => { if (current === generation) { error.value = failure instanceof Error ? failure.message : String(failure); validateWith(error.value); } })
        .finally(() => { if (current === generation) { converting.value = false; publishDraft(); } });
    }, 180);
  }
  function change(patch: Partial<McpDraftState>, touched?: string) {
    if (props.readonly || loading.value || !connection.state.value) return;
    Object.assign(connection.state.value, patch); if (touched) connection.touch(touched); update();
  }
  function setField(key: string, value: McpFieldDraft, option = false) {
    if (props.readonly || loading.value) return;
    connection.setField(key, value, option); update();
  }
  function setTransport(value: string) {
    if (props.readonly || loading.value) return;
    connection.setTransport(value); update();
  }
  function showRaw() {
    if (loading.value) return;
    cancel(); mode.value = "raw"; rawAtSwitch = lastEmission ?? props.modelValue; rawChanged.value = false;
    error.value = ""; validateWith(rawNotice.value); publishDraft();
  }
  function setRaw(text: string) {
    if (props.readonly) return;
    cancel(); error.value = ""; rawChanged.value = text !== rawAtSwitch; emit(text);
    validateWith(rawNotice.value); publishDraft();
  }
  function showForm() {
    if (connection.state.value && !rawChanged.value) {
      cancel(); mode.value = "form"; error.value = "";
      if (pendingForm.value) update(); else validateWith(firstError.value);
      publishDraft();
    } else void read(props.modelValue);
  }
  function useRaw() {
    // Explicit choice discards the incomplete form only after parsing succeeds.
    void read(props.modelValue);
  }
  watch(() => [props.modelValue, props.agentKind, props.format] as const, ([text], previous) => {
    if (text === lastEmission && previous?.[1] === props.agentKind && previous?.[2] === props.format) return;
    cancel(); lastEmission = null;
    const saved = !previous ? props.draft : undefined;
    if (saved) {
      connection.load(saved.schema, saved.state, saved.baseline); sourceText = saved.sourceText;
      mode.value = saved.mode; rawAtSwitch = saved.rawAtSwitch; rawChanged.value = saved.rawChanged; pendingForm.value = saved.pendingForm;
      validateWith(saved.validation);
      if (mode.value === "form" && pendingForm.value) update();
    } else void read(text);
  }, { immediate: true });
  onScopeDispose(cancel);
  return { connection, mode, loading, converting, error, rawNotice, validation, firstError,
    setField, setTransport, change, showRaw, showForm, setRaw, useRaw };
}
