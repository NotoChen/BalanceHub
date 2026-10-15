<script setup lang="ts">
import { computed, type UnwrapNestedRefs } from "vue";
import type { useAgentConfigurationEditor } from "../../composables/useAgentConfigurationEditor";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentConfigurationEditorBody from "./AgentConfigurationEditorBody.vue";
import FileEditorModal from "../FileEditorModal.vue";
import type { AgentConfigurationSource } from "../../stores/agent-configuration-types";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import { agentConfigurationFileLabel } from "../../utils/agent-configuration-display";

const props = defineProps<{
  model: UnwrapNestedRefs<ReturnType<typeof useAgentConfigurationEditor>>;
  sources?: AgentConfigurationSource[];
}>();
const emit = defineEmits<{ openSource: [sourceId: string] }>();
const choices = computed(() =>
  (props.sources ?? []).filter(
    (source) =>
      source.visualSettings &&
      source.access.kind === "ready" &&
      source.actions.some(
        (action) =>
          action.available && ["edit", "create"].includes(action.action),
      ),
  ),
);
function label(source: AgentConfigurationSource) {
  const peers = choices.value.filter((item) => item.scope === source.scope);
  return [
    agentAssetScopeLabels[source.scope],
    source.profile ||
      (peers.length > 1 ? agentConfigurationFileLabel(source, peers) : ""),
  ]
    .filter(Boolean)
    .join(" · ");
}
</script>

<template>
  <FileEditorModal :visible="model.visible" fill @close="model.close">
    <template #title
      ><div class="surface-modal-title">
        <AgentCliIcon
          v-if="model.edit"
          :kind="model.edit.agentKind"
          :size="20"
        /><strong>{{ model.planVisible ? "确认保存" : model.title }}</strong>
      </div></template
    >
    <nav
      v-if="
        !model.planVisible &&
        model.edit?.documents.some((document) => document.visualSettings) &&
        choices.length > 1
      "
      class="agent-settings-source-tabs"
      aria-label="配置保存范围"
    >
      <button
        v-for="source in choices"
        :key="source.sourceId"
        type="button"
        :title="source.path"
        :aria-pressed="
          model.edit?.documents.some(
            (document) => document.sourceId === source.sourceId,
          )
        "
        @click="emit('openSource', source.sourceId)"
      >
        {{ label(source) }}
      </button>
    </nav>
    <AgentConfigurationEditorBody :model="model" @close="model.close"
      ><template #file-actions="slotProps"
        ><slot name="file-actions" v-bind="slotProps" /></template
    ></AgentConfigurationEditorBody>
  </FileEditorModal>
</template>
