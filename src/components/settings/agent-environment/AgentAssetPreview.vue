<script setup lang="ts">
import { computed } from "vue";
import { IconLoading } from "@arco-design/web-vue/es/icon";
import type { AgentAssetReadDiagnostic, AgentAssetReadResult } from "../../../stores/provider-types";
import type { AgentAssetPreviewState } from "../../../composables/useAgentAssetConsole";
import CodeEditor from "../../CodeEditor.vue";
import { codeFormatForPath } from "../../../utils/code-editor-config";

const props = defineProps<{ preview: AgentAssetReadResult | null; state: AgentAssetPreviewState; error: string }>();
const labels: Record<AgentAssetReadDiagnostic, string> = {
  directoryMetadataOnly: "目录仅提供元数据",
  sensitiveFileMetadataOnly: "凭据文件仅提供元数据",
  sensitiveValuesRedacted: "敏感字段已隐藏",
  unsupportedSchemaMetadataOnly: "此配置格式尚不支持安全预览，仅展示元数据",
  invalidDocumentMetadataOnly: "配置内容无法解析，仅展示元数据",
  readLimitMetadataOnly: "来源超过读取上限，仅展示元数据",
};
const diagnostics = computed(() => props.preview?.diagnostics.map((item) => labels[item]) ?? []);
</script>

<template>
  <div class="agent-config-preview">
    <div v-if="state === 'loading'" class="agent-config-preview-empty" role="status"><IconLoading class="agent-version-loading" /> 正在读取配置预览</div>
    <div v-else-if="state === 'error'" class="agent-config-preview-empty is-error" role="status">{{ error }}</div>
    <template v-else-if="preview">
      <header class="agent-config-preview-head"><span :title="preview.path">{{ preview.path }}</span><span>{{ preview.metadataOnly ? "仅元数据" : preview.truncated ? "内容已截断" : `${preview.sizeBytes} B` }}</span></header>
      <CodeEditor v-if="preview.content !== null" :model-value="preview.content" :format="codeFormatForPath(preview.path)" readonly />
      <div v-else class="agent-config-preview-empty">{{ preview.metadataOnly ? "此来源仅提供元数据" : "没有可预览的内容" }}</div>
      <p v-for="diagnostic in diagnostics" :key="diagnostic" class="agent-config-diagnostic">{{ diagnostic }}</p>
    </template>
    <div v-else class="agent-config-preview-empty">尚未读取配置内容</div>
  </div>
</template>
