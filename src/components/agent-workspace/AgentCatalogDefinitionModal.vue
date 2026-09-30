<script setup lang="ts">
import type { AgentCatalogDefinitionFormProps } from "../../composables/useAgentDefinitionEditor";
import type { AgentCatalogSaveRequest } from "../../stores/agent-catalog-types";
import { agentAssetCategoryLabels } from "../../composables/useAgentAssetCatalog";
import AgentCatalogDefinitionForm from "./AgentCatalogDefinitionForm.vue";
import FileEditorModal from "../FileEditorModal.vue";

const props = defineProps<AgentCatalogDefinitionFormProps>();
const emit = defineEmits<{ close: []; retry: []; save: [request: AgentCatalogSaveRequest]; apply: [request: AgentCatalogSaveRequest] }>();
</script>

<template>
  <FileEditorModal :visible="visible" @close="emit('close')">
    <template #title>{{ editingAssetId ? '编辑' : '新建' }} {{ agentAssetCategoryLabels[category] }}</template>
    <AgentCatalogDefinitionForm v-if="visible" v-bind="props" @close="emit('close')" @retry="emit('retry')" @save="emit('save', $event)" @apply="emit('apply', $event)" />
  </FileEditorModal>
</template>
