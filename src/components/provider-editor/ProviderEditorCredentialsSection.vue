<script setup lang="ts">
import { computed } from "vue";
import { IconCopy } from "@arco-design/web-vue/es/icon";
import type { ProviderInput } from "../../stores/providers";

const props = defineProps<{
  draft: ProviderInput;
}>();

const emit = defineEmits<{
  "copy-api-key": [];
}>();

const apiUserReadonly = computed(() => props.draft.auth.mode === "session");
</script>

<template>
  <a-form-item field="auth.sessionCookie" label="会话 Cookie" :required="draft.auth.mode === 'session'">
    <a-input-password v-model="draft.auth.sessionCookie" placeholder="session=xxx 或 xxx" />
    <template #extra>
      <span v-if="draft.auth.mode === 'session'">当前优先使用 Cookie 认证。</span>
      <span v-else>可选。填写后可用于账号额度、签到、API Key 管理等账号能力。</span>
    </template>
  </a-form-item>

  <a-form-item field="auth.accessToken" label="访问令牌" :required="draft.auth.mode === 'accessToken'">
    <a-input-password v-model="draft.auth.accessToken" placeholder="访问令牌" />
    <template #extra>
      <span v-if="draft.auth.mode === 'accessToken'">当前优先使用访问令牌认证。</span>
      <span v-else>可选。Cookie 不可用时可作为账号能力的备用凭据。</span>
    </template>
  </a-form-item>

  <a-form-item
    field="auth.apiUser"
    label="API User ID"
    :required="draft.auth.mode === 'session' || draft.auth.mode === 'accessToken'"
  >
    <a-input
      v-model="draft.auth.apiUser"
      :placeholder="apiUserReadonly ? '填写 Cookie 后自动解析' : '用户 ID'"
      :readonly="apiUserReadonly"
    />
    <template #extra>
      <span v-if="apiUserReadonly">由会话 Cookie 自动解析。</span>
      <span v-else>访问令牌、签到和 API Key 管理通常需要用户 ID。</span>
    </template>
  </a-form-item>

  <a-form-item field="auth.apiKey" label="API 密钥" :required="draft.auth.mode === 'apiKey'">
    <div class="input-action-row">
      <a-input-password v-model="draft.auth.apiKey" placeholder="sk-..." />
      <a-button :disabled="!draft.auth.apiKey.trim()" @click="emit('copy-api-key')">
        <template #icon><icon-copy /></template>
      </a-button>
    </div>
    <template #extra>
      <span v-if="draft.auth.mode === 'apiKey'">当前优先使用 API 密钥认证。</span>
      <span v-else>可选。API 密钥不支持签到，仅支持查询该密钥自身额度和测活。</span>
    </template>
  </a-form-item>
</template>
