<script setup lang="ts">
import { computed } from "vue";
import { IconCommand } from "@arco-design/web-vue/es/icon";
import { brandSource } from "../agent-cli/visuals";

const props = withDefaults(
  defineProps<{
    kind: string;
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

const source = computed(() => brandSource(props.kind));
</script>

<template>
  <img
    v-if="source"
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
  <IconCommand
    v-else
    class="brand-icon agent-cli-icon agent-cli-icon-generic"
    :style="{ width: `${size}px`, height: `${size}px` }"
    :aria-hidden="decorative || undefined"
    :aria-label="decorative ? undefined : label"
    :role="decorative ? undefined : 'img'"
  />
</template>
