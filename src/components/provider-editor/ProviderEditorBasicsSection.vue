<script setup lang="ts">
import type { ProviderInput } from "../../stores/providers";
import { authModeOptions, providerKindOptions } from "./options";

defineProps<{
  draft: ProviderInput;
}>();
</script>

<template>
  <a-form-item field="providerKind" label="中转站类型" required>
    <a-select v-model="draft.identity.providerKind" :options="providerKindOptions" disabled />
  </a-form-item>
  <a-form-item field="baseUrl" label="中转站地址" required>
    <a-input v-model="draft.identity.baseUrl" placeholder="https://relay.example.com" />
  </a-form-item>
  <a-form-item field="authMode" label="认证方式" required>
    <a-select v-model="draft.auth.mode" :options="authModeOptions" />
  </a-form-item>
</template>
