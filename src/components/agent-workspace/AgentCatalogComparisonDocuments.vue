<script setup lang="ts">
import type { AgentCatalogComparisonDocument } from "../../stores/agent-catalog-types";
import { ref } from "vue";
import CodeEditor from "../CodeEditor.vue";
import { codeFormatForPath } from "../../utils/code-editor-config";
defineProps<{ documents: AgentCatalogComparisonDocument[] }>();
const expanded = ref(new Set<string>());
function toggle(key: string, event: Event) {
  if ((event.target as HTMLDetailsElement).open) expanded.value.add(key);
  else expanded.value.delete(key);
}
</script>

<template>
  <details v-for="document in documents" :key="document.key" class="agent-catalog-comparison-document" @toggle="toggle(document.key, $event)">
    <summary>{{ document.label }}<small v-if="document.truncated">部分内容</small></summary>
    <code v-if="document.path">{{ document.path }}</code>
    <p v-if="document.reason" class="agent-workspace-note">{{ document.reason }}</p>
    <CodeEditor v-if="document.content !== null && expanded.has(document.key)" :model-value="document.content" :format="document.format === 'text' ? codeFormatForPath(document.path) : document.format === 'binary' ? 'text' : document.format" readonly />
    <p v-else-if="document.content === null" class="agent-workspace-note">{{ document.format === 'binary' ? '二进制文件，按后端完整内容结果比较' : '当前没有可展示的内容' }}</p>
  </details>
</template>
