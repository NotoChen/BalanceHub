<script setup lang="ts">
import { computed } from "vue";
import { Cookie, Fingerprint, KeyRound } from "@lucide/vue";
import type { AuthMode } from "../stores/providers";

const props = withDefaults(
  defineProps<{
    mode: AuthMode;
    size?: number;
    strokeWidth?: number;
    decorative?: boolean;
  }>(),
  {
    size: 15,
    strokeWidth: 1.8,
    decorative: true,
  },
);

const icon = computed(() => {
  if (props.mode === "session") {
    return Cookie;
  }
  if (props.mode === "accessToken") {
    return Fingerprint;
  }
  return KeyRound;
});

const label = computed(() => {
  if (props.mode === "session") {
    return "Cookie";
  }
  if (props.mode === "accessToken") {
    return "访问令牌";
  }
  return "API Key";
});
</script>

<template>
  <component
    :is="icon"
    :size="size"
    :stroke-width="strokeWidth"
    :aria-hidden="decorative || undefined"
    :aria-label="decorative ? undefined : label"
    :title="decorative ? undefined : label"
  />
</template>
