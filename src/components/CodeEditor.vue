<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from "vue";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, drawSelection, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching, foldGutter, foldKeymap, indentOnInput } from "@codemirror/language";
import { closeSearchPanel, highlightSelectionMatches, openSearchPanel, search, searchKeymap, searchPanelOpen } from "@codemirror/search";
import { codeHighlighting, codeLanguage, codePhrases, type CodeFormat } from "../utils/code-editor-config";
import "../styles/modules/code-editor.css";

const props = withDefaults(defineProps<{ modelValue: string; format?: CodeFormat; readonly?: boolean; wrap?: boolean; label?: string }>(), {
  format: "text", readonly: false, wrap: false, label: "文件内容",
});
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const host = ref<HTMLElement | null>(null);
const line = ref(1);
const column = ref(1);
const lines = ref(1);
const language = new Compartment();
const editable = new Compartment();
const wrapping = new Compartment();
const lineSeparator = new Compartment();
let view: EditorView | null = null;
const access = () => [EditorState.readOnly.of(props.readonly), EditorView.editable.of(!props.readonly), EditorView.contentAttributes.of({ "aria-label": props.label, "aria-readonly": String(props.readonly), spellcheck: "false", tabindex: "0" })];
const separator = () => EditorState.lineSeparator.of(props.modelValue.includes("\r\n") ? "\r\n" : "\n");
function updatePosition(state: EditorState) {
  const current = state.doc.lineAt(state.selection.main.head);
  line.value = current.number;
  column.value = state.selection.main.head - current.from + 1;
  lines.value = state.doc.lines;
}
onMounted(() => {
  if (!host.value) return;
  view = new EditorView({
    parent: host.value,
    state: EditorState.create({ doc: props.modelValue, extensions: [
      lineNumbers(), foldGutter(), highlightActiveLineGutter(), history(), drawSelection(), highlightActiveLine(),
      bracketMatching(), indentOnInput(), highlightSelectionMatches(), search({ top: true }),
      keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, ...foldKeymap]),
      codeHighlighting, codePhrases, EditorState.tabSize.of(2),
      language.of(codeLanguage(props.format)), editable.of(access()),
      wrapping.of(props.wrap ? EditorView.lineWrapping : []), lineSeparator.of(separator()),
      EditorView.updateListener.of((update) => {
        if (update.docChanged || update.selectionSet) updatePosition(update.state);
        if (update.docChanged && update.state.sliceDoc() !== props.modelValue) emit("update:modelValue", update.state.sliceDoc());
      }),
    ] }),
  });
  updatePosition(view.state);
});
watch(() => props.modelValue, (text) => {
  if (!view || view.state.sliceDoc() === text) return;
  const document = EditorState.create({ doc: text, extensions: [separator()] }).doc;
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: document }, effects: lineSeparator.reconfigure(separator()) });
});
watch(() => props.format, () => view?.dispatch({ effects: language.reconfigure(codeLanguage(props.format)) }));
watch(() => [props.readonly, props.label], () => view?.dispatch({ effects: editable.reconfigure(access()) }));
watch(() => props.wrap, () => view?.dispatch({ effects: wrapping.reconfigure(props.wrap ? EditorView.lineWrapping : []) }));
onBeforeUnmount(() => { view?.destroy(); view = null; });
function keydown(event: KeyboardEvent) {
  if (!view) return;
  if (event.key === "Escape" && searchPanelOpen(view.state)) {
    event.preventDefault(); event.stopPropagation(); closeSearchPanel(view);
  } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
    event.preventDefault(); event.stopPropagation(); openSearchPanel(view);
  }
}
defineExpose({ search: () => { if (view) openSearchPanel(view); } });
</script>

<template>
  <div class="code-editor" :class="{ 'is-readonly': readonly }" @keydown.capture="keydown">
    <div ref="host" class="code-editor-host" />
    <footer class="code-editor-status"><span>{{ readonly ? '只读' : '可编辑' }}</span><span>第 {{ line }} 行，第 {{ column }} 列</span><span>{{ lines }} 行</span><span>{{ format === 'dotenv' ? 'ENV' : format.toUpperCase() }}</span></footer>
  </div>
</template>
