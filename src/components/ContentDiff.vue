<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { buildConfigurationDiff } from "../utils/configuration-text-diff";
import "../styles/modules/configuration-text-editor.css";

const props = withDefaults(defineProps<{ originalText: string; modifiedText: string; contextLines?: number }>(), { contextLines: 3 });
const expanded = ref(false);
const diff = computed(() => buildConfigurationDiff(props.originalText, props.modifiedText));
const changes = computed(() => ({
  added: diff.value.lines.filter((line) => line.kind === "added").length,
  removed: diff.value.lines.filter((line) => line.kind === "removed").length,
}));
const lineEndingNote = computed(() => {
  if (props.originalText !== props.modifiedText && props.originalText.replace(/\r\n/g, "\n") === props.modifiedText.replace(/\r\n/g, "\n")) {
    return "文本行内容相同，换行符格式不同（LF / CRLF）。";
  }
  return "";
});
const finalNewlineNote = computed(() => {
  if (props.originalText.endsWith("\n") === props.modifiedText.endsWith("\n")) return "";
  return props.modifiedText.endsWith("\n") ? "+ 文件末尾新增换行" : "− 文件末尾的换行被移除";
});
const rows = computed(() => {
  const lines = diff.value.lines;
  if (props.contextLines === undefined || expanded.value) return lines;
  const context = Math.max(0, props.contextLines);
  const visible = new Set<number>();
  lines.forEach((line, index) => {
    if (line.kind === "context") return;
    for (let offset = Math.max(0, index - context); offset <= Math.min(lines.length - 1, index + context); offset += 1) visible.add(offset);
  });
  const result: Array<typeof lines[number] | { id: string; kind: "omitted"; count: number }> = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (visible.has(index)) result.push(lines[index]);
    else {
      const start = index;
      while (index + 1 < lines.length && !visible.has(index + 1)) index += 1;
      result.push({ id: `omitted-${start}`, kind: "omitted", count: index - start + 1 });
    }
  }
  return result;
});
watch(() => [props.originalText, props.modifiedText], () => { expanded.value = false; });
</script>

<template>
  <div class="configuration-diff" aria-label="文本差异">
    <div class="configuration-diff-summary"><span class="configuration-diff-added-count">+{{ changes.added }}</span><span class="configuration-diff-removed-count">−{{ changes.removed }}</span><span>行</span></div>
    <p v-if="lineEndingNote" class="configuration-diff-note">{{ lineEndingNote }}</p>
    <div class="configuration-diff-scroll">
      <template v-for="line in rows" :key="line.id">
        <button v-if="line.kind === 'omitted'" type="button" class="configuration-diff-omitted" @click="expanded = true">展开 {{ line.count }} 行相同内容</button>
        <div v-else class="configuration-diff-line" :class="'configuration-diff-line-' + line.kind">
          <span class="configuration-diff-marker" aria-hidden="true">{{ line.kind === 'removed' ? '−' : line.kind === 'added' ? '+' : ' ' }}</span>
          <span class="configuration-diff-number">{{ line.oldLine ?? '' }}</span>
          <span class="configuration-diff-number">{{ line.newLine ?? '' }}</span>
          <code class="configuration-diff-text">{{ line.text || ' ' }}</code>
        </div>
      </template>
      <p v-if="!diff.lines.length" class="configuration-diff-note">空文件</p>
      <p v-if="finalNewlineNote" class="configuration-diff-note">{{ finalNewlineNote }}</p>
    </div>
  </div>
</template>
