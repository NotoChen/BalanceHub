<script setup lang="ts">
import { computed } from "vue";
import { Cookie, Fingerprint, KeyRound, UserRoundKey } from "@lucide/vue";
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
  if (props.mode === "password") {
    return UserRoundKey;
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
  if (props.mode === "password") {
    return "账号密码";
  }
  return "API Key";
});

const modeClass = computed(() => {
  if (props.mode === "accessToken") {
    return "provider-auth-icon-access-token";
  }
  if (props.mode === "apiKey") {
    return "provider-auth-icon-api-key";
  }
  if (props.mode === "password") {
    return "provider-auth-icon-password";
  }
  return "provider-auth-icon-session";
});
</script>

<template>
  <component
    :is="icon"
    class="provider-auth-icon"
    :class="modeClass"
    :size="size"
    :stroke-width="strokeWidth"
    :aria-hidden="decorative || undefined"
    :aria-label="decorative ? undefined : label"
    :title="decorative ? undefined : label"
  />
</template>
