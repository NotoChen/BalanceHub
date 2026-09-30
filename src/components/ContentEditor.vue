<script setup lang="ts">
import { computed, nextTick, onScopeDispose, ref, watch } from "vue";
import CodeEditor from "./CodeEditor.vue";
import ContentDiff from "./ContentDiff.vue";
import { formatConfigurationText } from "../utils/configuration-format";
import type { AgentConfigurationFormat } from "../stores/agent-configuration-types";
import { copyText } from "../composables/useClipboard";
import { withTimeout } from "../utils/promise-timeout";
import "../styles/modules/configuration-text-editor.css";

const props = withDefaults(defineProps<{ originalText: string; modelValue: string; format?: AgentConfigurationFormat; readonly?: boolean; label?: string }>(), { readonly: false });
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const showingDiff = ref(false);
const wrapping = ref(false);
const editor = ref<InstanceType<typeof CodeEditor> | null>(null);
const displayDiff = computed(() => showingDiff.value || (props.readonly && props.originalText !== props.modelValue));
const copied = ref(false);
const copyError = ref("");
const formatting = ref(false);
const formatError = ref("");
let copyRequest = 0;
let formatRequest = 0;
watch(() => [props.modelValue, props.originalText, props.format, props.readonly], () => {
  copyRequest += 1; copied.value = false; copyError.value = "";
  formatRequest += 1; formatting.value = false; formatError.value = "";
}, { flush: "sync" });
onScopeDispose(() => { copyRequest += 1; formatRequest += 1; });
async function formatDraft() {
  if (props.readonly || !props.format || formatting.value) return;
  const request = ++formatRequest;
  formatting.value = true;
  formatError.value = "";
  try {
    const result = await withTimeout(formatConfigurationText(props.format, props.modelValue), 15_000, "格式化超时，原文未修改");
    if (request === formatRequest) { showingDiff.value = false; emit("update:modelValue", result); }
  } catch (failure) {
    if (request === formatRequest) formatError.value = failure instanceof Error ? failure.message : "格式化失败，请检查文件语法";
  } finally {
    if (request === formatRequest) formatting.value = false;
  }
}
async function copyDraft() {
  const request = ++copyRequest;
  try { await withTimeout(copyText(props.modelValue), 5000, "复制超时"); if (request === copyRequest) copied.value = true; }
  catch { if (request === copyRequest) copyError.value = "复制失败，请重试"; }
}
async function search() { showingDiff.value = false; await nextTick(); editor.value?.search(); }
</script>

<template>
  <div class="content-editor">
    <div class="content-editor-toolbar">
      <div v-if="!readonly" class="content-editor-tabs" role="group" aria-label="编辑器视图">
        <button type="button" :aria-pressed="!showingDiff" @click="showingDiff = false">内容</button>
        <button type="button" :aria-pressed="showingDiff" @click="showingDiff = true">差异</button>
      </div>
      <span v-if="!readonly && originalText !== modelValue" class="content-editor-dirty">已修改</span>
      <div class="content-editor-actions">
        <a-button v-if="!readonly && format" size="mini" type="text" :loading="formatting" @click="formatDraft">格式化</a-button>
        <a-button size="mini" type="text" :disabled="displayDiff && readonly" @click="search">查找</a-button>
        <a-button size="mini" type="text" :disabled="displayDiff" :aria-pressed="wrapping" @click="wrapping = !wrapping">自动换行</a-button>
        <a-button size="mini" type="text" @click="copyDraft">{{ copied ? '已复制' : '复制内容' }}</a-button>
      </div>
    </div>
    <p v-if="copyError" class="agent-workspace-error">{{ copyError }}</p>
    <p v-if="formatError" class="agent-workspace-error" role="alert">{{ formatError }}</p>
    <CodeEditor v-show="!displayDiff" ref="editor" :model-value="modelValue" :format="format" :label="label" :readonly="readonly" :wrap="wrapping" @update:model-value="!readonly && emit('update:modelValue', $event)" />
    <ContentDiff v-if="displayDiff" :original-text="originalText" :modified-text="modelValue" />
  </div>
</template>
