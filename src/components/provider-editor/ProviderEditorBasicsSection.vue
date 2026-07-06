<script setup lang="ts">
import type { ProviderInput } from "../../stores/providers";
import { authModeOptions } from "./options";

defineProps<{
  draft: ProviderInput;
}>();
</script>

<template>
  <a-form-item field="identity.baseUrl" label="中转站地址" required>
    <a-input v-model="draft.identity.baseUrl" placeholder="https://relay.example.com" />
  </a-form-item>
  <a-form-item field="auth.mode" label="优先认证方式" required>
    <a-select v-model="draft.auth.mode" :options="authModeOptions" />
    <template #extra>用于决定请求时优先使用哪种凭据，不会隐藏其他已配置凭据。</template>
  </a-form-item>
</template>
