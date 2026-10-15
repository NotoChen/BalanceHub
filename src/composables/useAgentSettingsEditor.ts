import { computed, onScopeDispose, ref, shallowRef, watch } from "vue";
import { readAgentConfigurationForm } from "../api/agent-configuration";
import type {
  AgentConfigurationForm,
  AgentNativeSetting,
} from "../stores/agent-settings-types";
import type { AgentConfigurationFormat } from "../stores/agent-configuration-types";
import {
  agentConfigurationErrorDiagnostics,
  agentConfigurationErrorMessage,
} from "../utils/agent-configuration-display";
import { withTimeout } from "../utils/promise-timeout";

export interface AgentSettingsEditorDraft {
  form: AgentConfigurationForm | null;
  sourceText: string;
  changes: Record<string, string | null>;
  mode: "form" | "raw";
  pending: boolean;
  rawAtSwitch: string;
  rawChanged: boolean;
  modified: boolean;
  validation: string;
}

export interface AgentSettingsEditorProps {
  editId: string;
  revision: string;
  sourceId: string;
  modelValue: string;
  originalText: string;
  format: AgentConfigurationFormat;
  readonly?: boolean;
  draft?: AgentSettingsEditorDraft;
}

export function useAgentSettingsEditor(
  props: Readonly<AgentSettingsEditorProps>,
  emitText: (text: string) => void,
  emitDraft: (draft: AgentSettingsEditorDraft) => void,
) {
  const form = shallowRef<AgentConfigurationForm | null>(null);
  const changes = ref<Record<string, string | null>>({});
  const mode = ref<"form" | "raw">("form");
  const loading = ref(false);
  const converting = ref(false);
  const error = ref("");
  const validation = ref("");
  const pending = computed(() => Object.keys(changes.value).length > 0);
  const rawChanged = ref(false);
  let sourceText = "";
  let rawAtSwitch = "";
  let generation = 0;
  let lastEmission: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const rawNotice = computed(() =>
    mode.value === "raw" &&
    pending.value &&
    !rawChanged.value &&
    !converting.value
      ? "表单有尚未同步的修改。此处显示最后一次有效原文；返回可视化可继续编辑。"
      : "",
  );

  function cancel() {
    generation++;
    clearTimeout(timer);
    timer = undefined;
    loading.value = false;
    converting.value = false;
  }
  function errorMessage(cause: unknown, fallback: string) {
    return (
      agentConfigurationErrorDiagnostics(cause)
        .map((item) => item.message)
        .join("；") || agentConfigurationErrorMessage(cause, fallback)
    );
  }
  function publish() {
    emitDraft({
      form: form.value,
      sourceText,
      changes: { ...changes.value },
      mode: mode.value,
      pending: pending.value,
      rawAtSwitch,
      rawChanged: rawChanged.value,
      modified:
        pending.value ||
        (lastEmission ?? props.modelValue) !== props.originalText,
      validation: validation.value,
    });
  }
  function emit(text: string) {
    lastEmission = text;
    emitText(text);
  }
  function valueFor(id: string): string | undefined {
    return Object.prototype.hasOwnProperty.call(changes.value, id)
      ? (changes.value[id] ?? undefined)
      : form.value?.values[id];
  }
  function visible(field: AgentNativeSetting) {
    if (valueFor(field.id) !== undefined) return true;
    const current = field.whenOption
      ? (valueFor(field.whenOption[0]) ??
        form.value?.fields.find((item) => item.id === field.whenOption![0])
          ?.currentValue)
      : undefined;
    return !field.whenOption || current === field.whenOption[1];
  }
  async function request(text: string, patch: Record<string, string | null>) {
    return withTimeout(
      readAgentConfigurationForm({
        editId: props.editId,
        sourceId: props.sourceId,
        expectedRevision: props.revision,
        text,
        changes: patch,
      }),
      15_000,
      "读取配置表单超时，草稿已保留",
    );
  }
  async function read(text: string, nextMode: "form" | "raw" = "form") {
    cancel();
    const current = generation;
    loading.value = true;
    error.value = "";
    form.value = null;
    try {
      const result = await request(text, {});
      if (current !== generation) return null;
      form.value = result;
      sourceText = text;
      changes.value = {};
      mode.value = nextMode;
      rawAtSwitch = text;
      rawChanged.value = false;
      validation.value = "";
      publish();
      return result;
    } catch (cause) {
      if (current !== generation) return null;
      error.value = errorMessage(
        cause,
        "无法解析可视化字段，可在原文中修正后重试",
      );
      mode.value = "raw";
      rawAtSwitch = sourceText;
      rawChanged.value = text !== sourceText;
      validation.value = rawNotice.value;
      publish();
      return null;
    } finally {
      if (current === generation) loading.value = false;
    }
  }
  function update() {
    cancel();
    error.value = "";
    validation.value = "";
    if (!pending.value) {
      publish();
      return;
    }
    const current = generation;
    const patch = { ...changes.value };
    converting.value = true;
    publish();
    timer = setTimeout(() => {
      timer = undefined;
      void request(sourceText, patch)
        .then((result) => {
          if (current !== generation) return;
          form.value = result;
          sourceText = result.text;
          changes.value = {};
          emit(result.text);
          validation.value = "";
        })
        .catch((cause) => {
          if (current === generation) {
            error.value = errorMessage(
              cause,
              "无法更新原生配置草稿，请检查填写值",
            );
            validation.value = error.value;
          }
        })
        .finally(() => {
          if (current === generation) {
            converting.value = false;
            publish();
          }
        });
    }, 160);
  }
  function change(id: string, value: string | undefined) {
    if (props.readonly || loading.value || mode.value !== "form") return;
    if (value === form.value?.values[id]) delete changes.value[id];
    else changes.value[id] = value ?? null;
    const group = form.value?.fields.find(
      (field) => field.id === id,
    )?.exclusiveGroup;
    for (const field of form.value?.fields ?? []) {
      if (field.whenOption?.[0] === id && field.whenOption[1] !== value)
        delete changes.value[field.id];
      if (
        value !== undefined &&
        group &&
        field.id !== id &&
        field.exclusiveGroup === group
      ) {
        if (form.value?.values[field.id] !== undefined)
          changes.value[field.id] = null;
        else delete changes.value[field.id];
      }
    }
    update();
  }
  async function showRaw() {
    cancel();
    mode.value = "raw";
    rawAtSwitch = lastEmission ?? props.modelValue;
    rawChanged.value = false;
    error.value = "";
    validation.value = rawNotice.value;
    if (!pending.value || props.readonly) {
      publish();
      return form.value;
    }
    const current = generation;
    converting.value = true;
    publish();
    try {
      const result = await request(sourceText, { ...changes.value });
      if (current !== generation) return null;
      form.value = result;
      sourceText = result.text;
      changes.value = {};
      rawAtSwitch = result.text;
      emit(result.text);
      validation.value = "";
      return result;
    } catch (cause) {
      if (current !== generation) return null;
      error.value = errorMessage(
        cause,
        "表单修改暂未同步，可返回可视化继续编辑",
      );
      validation.value = error.value;
      return null;
    } finally {
      if (current === generation) {
        converting.value = false;
        publish();
      }
    }
  }
  function setRaw(text: string) {
    if (props.readonly) return;
    cancel();
    rawChanged.value = text !== rawAtSwitch;
    emit(text);
    if (rawChanged.value) {
      changes.value = {};
      form.value = null;
    }
    error.value = "";
    validation.value = rawNotice.value;
    publish();
  }
  async function showForm() {
    if (
      form.value &&
      !rawChanged.value &&
      sourceText === (lastEmission ?? props.modelValue)
    ) {
      mode.value = "form";
      update();
      return form.value;
    }
    return read(lastEmission ?? props.modelValue);
  }
  function readForNavigation() {
    const text = lastEmission ?? props.modelValue;
    // Cursor movement uses the existing locations. Only an explicit navigation
    // click reparses an edited raw draft, retaining its current editor view.
    if (form.value?.text === text) return Promise.resolve(form.value);
    return read(text, "raw");
  }
  function restore(saved: AgentSettingsEditorDraft) {
    form.value = saved.form;
    sourceText = saved.sourceText;
    changes.value = { ...saved.changes };
    mode.value = saved.mode;
    rawAtSwitch = saved.rawAtSwitch;
    rawChanged.value = saved.rawChanged;
    validation.value = saved.validation;
    error.value = saved.validation;
    if (mode.value === "form" && pending.value && !props.readonly) update();
  }
  watch(
    () =>
      [props.editId, props.sourceId, props.revision, props.modelValue] as const,
    ([, , , text], previous) => {
      if (
        text === lastEmission &&
        previous?.[0] === props.editId &&
        previous?.[1] === props.sourceId &&
        previous?.[2] === props.revision
      )
        return;
      cancel();
      lastEmission = null;
      const saved = props.draft;
      if (
        saved &&
        (!previous || saved.sourceText === text || saved.mode === "raw")
      )
        restore(saved);
      else void read(text);
    },
    { immediate: true },
  );
  watch(
    () => props.readonly,
    (readonly) => {
      if (readonly) cancel();
    },
  );
  onScopeDispose(cancel);
  return {
    form,
    mode,
    loading,
    converting,
    error,
    pending,
    rawNotice,
    validation,
    valueFor,
    visible,
    change,
    showRaw,
    setRaw,
    showForm,
    readForNavigation,
    useRaw: () => read(props.modelValue),
    retry: () =>
      pending.value && !rawChanged.value
        ? mode.value === "raw"
          ? showRaw()
          : update()
        : read(props.modelValue),
  };
}
