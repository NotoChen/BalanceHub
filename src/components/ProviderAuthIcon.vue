<script setup lang="ts">
import { computed } from "vue";
import { Cookie, Fingerprint, KeyRound, Ticket, UserRoundKey } from "@lucide/vue";
import type { AuthMode, ProviderProtocol } from "../stores/providers";

const props = withDefaults(
  defineProps<{
    mode: AuthMode;
    protocol?: ProviderProtocol;
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

// Sub2API 的凭据是 JWT（Access + Refresh），语义不同于 NewAPI 的系统访问令牌，
// 单独用一个「Access Token」图标表达；后续 OAuth 落地时在此再加一类即可。
const isSub2ApiToken = computed(
  () => props.protocol === "sub2Api" && props.mode === "accessToken",
);

const icon = computed(() => {
  if (isSub2ApiToken.value) {
    return Ticket;
  }
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
  if (isSub2ApiToken.value) {
    return "Access Token";
  }
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
