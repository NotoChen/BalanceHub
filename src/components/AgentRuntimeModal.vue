<script setup lang="ts">
import { computed } from "vue";
import { Terminal } from "@lucide/vue";
import type { AgentCliKind, CliRuntimeSnapshot, Provider, AgentRuntimeSession } from "../stores/providers";
import { providerDisplayLabel } from "../utils/provider-display";
import AgentRuntimeList from "./AgentRuntimeList.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  cliKind: AgentCliKind | null;
  cliRuntime: CliRuntimeSnapshot;
  loading: boolean;
  instances: AgentRuntimeSession[];
  activatingId: string | null;
}>();
const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  activate: [instance: AgentRuntimeSession];
}>();
const title = computed(() => props.provider
  ? providerDisplayLabel(props.provider)
  : props.cliKind
    ? `${props.cliRuntime.agents.find((agent) => agent.kind === props.cliKind)?.label || props.cliKind} 活动会话`
    : "活动 Agent 会话");
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal temporary-cli-modal"
    :footer="false"
    :width="780"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title temporary-cli-modal-title">
        <span class="surface-modal-title-icon"><Terminal :size="18" :stroke-width="1.8" /></span>
        <span class="surface-modal-title-copy">
          <span>活动 Agent 会话</span>
          <strong>{{ title }}</strong>
        </span>
        <span class="surface-modal-title-meta" :class="{ ready: instances.length > 0 }">
          <i aria-hidden="true"></i>
          {{ instances.length }} 个活动会话
        </span>
      </div>
    </template>

    <AgentRuntimeList
      :provider="provider" :cli-kind="cliKind" :cli-runtime="cliRuntime"
      :loading="loading" :instances="instances" :activating-id="activatingId"
      @refresh="emit('refresh')" @activate="emit('activate', $event)"
    />
  </a-modal>
</template>
