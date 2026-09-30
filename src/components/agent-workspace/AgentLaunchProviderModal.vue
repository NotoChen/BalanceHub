<script setup lang="ts">
import { computed } from "vue";
import type { Provider } from "../../stores/provider-types";
import AgentProviderOptions from "./AgentProviderOptions.vue";
import "../../styles/modules/agent-launch-provider.css";

const props = defineProps<{ visible: boolean; agentLabel: string; providers: readonly Provider[] }>();
const emit = defineEmits<{ close: []; select: [providerId: string] }>();
const agentTitle = computed(() => props.agentLabel.trim() || "Agent");
</script>

<template>
  <a-modal
    :visible="visible"
    :title="`${agentTitle} · 选择中转站`"
    :width="620"
    :footer="false"
    modal-class="surface-modal agent-launch-provider-modal"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="emit('close')"
  >
    <div class="agent-launch-provider-picker">
      <div v-if="!providers.length" class="agent-launch-provider-empty" role="status">
        <strong>尚未添加中转站</strong>
        <p>请先在中转站视角添加中转站，再启动 {{ agentTitle }}。</p>
      </div>
      <AgentProviderOptions v-else :providers="providers" @select="emit('select', $event)" />
    </div>
  </a-modal>
</template>
