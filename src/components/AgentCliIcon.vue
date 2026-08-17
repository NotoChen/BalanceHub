<script setup lang="ts">
import { computed } from "vue";
import { agentCliVisuals, type AgentCliKind } from "../agent-cli/visuals";

const props = withDefaults(
  defineProps<{
    kind: AgentCliKind;
    size?: number;
    decorative?: boolean;
    label?: string;
  }>(),
  {
    size: 16,
    decorative: true,
    label: "Agent CLI",
  },
);

const source = computed(() => agentCliVisuals[props.kind].source);
</script>

<template>
  <img
    class="brand-icon agent-cli-icon"
    :class="`agent-cli-icon-${kind}`"
    :src="source"
    :width="size"
    :height="size"
    :alt="decorative ? '' : label"
    :aria-hidden="decorative || undefined"
    :title="decorative ? undefined : label"
    draggable="false"
  />
</template>
