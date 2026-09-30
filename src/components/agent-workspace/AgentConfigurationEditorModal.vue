<script setup lang="ts">
import type { UnwrapNestedRefs } from "vue";
import type { useAgentConfigurationEditor } from "../../composables/useAgentConfigurationEditor";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentConfigurationEditorBody from "./AgentConfigurationEditorBody.vue";
import FileEditorModal from "../FileEditorModal.vue";

defineProps<{ model: UnwrapNestedRefs<ReturnType<typeof useAgentConfigurationEditor>> }>();
</script>

<template>
  <FileEditorModal :visible="model.visible" fill @close="model.close">
    <template #title><div class="surface-modal-title"><AgentCliIcon v-if="model.edit" :kind="model.edit.agentKind" :size="20" /><strong>{{ model.planVisible ? '确认保存' : model.title }}</strong></div></template>
    <AgentConfigurationEditorBody :model="model" @close="model.close"><template #file-actions="slotProps"><slot name="file-actions" v-bind="slotProps" /></template></AgentConfigurationEditorBody>
  </FileEditorModal>
</template>
