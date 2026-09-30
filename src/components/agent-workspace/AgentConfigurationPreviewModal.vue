<script setup lang="ts">
import type { UnwrapNestedRefs } from "vue";
import type { useAgentConfiguration } from "../../composables/useAgentConfiguration";
import AgentConfigurationDiagnostics from "./AgentConfigurationDiagnostics.vue";
import AgentConfigurationFileActions from "./AgentConfigurationFileActions.vue";
import { agentConfigurationReadOnlyReason } from "../../utils/agent-configuration-display";
import FileEditorModal from "../FileEditorModal.vue";
import ContentEditor from "../ContentEditor.vue";
defineProps<{ model: UnwrapNestedRefs<ReturnType<typeof useAgentConfiguration>> }>();
</script>

<template>
  <FileEditorModal :visible="model.previewVisible" fill @close="model.closePreview">
    <template #title>{{ model.previewSource?.label }} · 只读</template>
    <div class="agent-configuration-editor-body">
      <code class="agent-configuration-path">{{ model.previewSource?.path }}</code>
      <AgentConfigurationFileActions :source="model.previewSource" :busy="Boolean(model.previewSource && model.opening[model.previewSource.sourceId])" @action="(source, action) => model.action(source.sourceId, action, source)" />
      <p v-if="model.previewSource" class="agent-workspace-note">{{ agentConfigurationReadOnlyReason(model.previewSource) }}</p>
      <p v-if="model.previewLoading" role="status">正在读取…</p>
      <p v-if="model.previewError" class="agent-workspace-error" role="alert">{{ model.previewError }}</p>
      <template v-if="model.preview"><p v-if="model.preview.truncated" class="agent-workspace-note">文件较大，仅显示部分内容。</p><ContentEditor :original-text="model.preview.text" :model-value="model.preview.text" :format="model.previewSource?.format" readonly /><AgentConfigurationDiagnostics :diagnostics="model.preview.diagnostics" /></template>
      <footer class="agent-configuration-editor-footer"><a-button @click="model.closePreview">关闭</a-button></footer>
    </div>
  </FileEditorModal>
</template>
