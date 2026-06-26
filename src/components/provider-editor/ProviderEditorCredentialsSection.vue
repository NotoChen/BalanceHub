<script setup lang="ts">
import { IconCopy } from "@arco-design/web-vue/es/icon";
import type { ProviderInput } from "../../stores/providers";

defineProps<{
  draft: ProviderInput;
}>();

const emit = defineEmits<{
  "copy-api-key": [];
}>();
</script>

<template>
  <template v-if="draft.auth.mode === 'session'">
    <a-form-item field="sessionCookie" label="会话 Cookie" required>
      <a-input-password v-model="draft.auth.sessionCookie" placeholder="session=xxx 或 xxx" />
    </a-form-item>
    <a-form-item field="apiUser" label="API User ID" required>
      <a-input v-model="draft.auth.apiUser" placeholder="填写 Cookie 后自动解析" readonly />
      <template #extra>由会话 Cookie 自动解析。</template>
    </a-form-item>
  </template>

  <template v-else-if="draft.auth.mode === 'accessToken'">
    <a-form-item field="accessToken" label="访问令牌" required>
      <a-input-password v-model="draft.auth.accessToken" placeholder="访问令牌" />
    </a-form-item>
    <a-form-item field="apiUser" label="API User ID" required>
      <a-input v-model="draft.auth.apiUser" placeholder="用户 ID" />
    </a-form-item>
  </template>

  <template v-else>
    <a-form-item field="apiKey" label="API 密钥" required>
      <div class="input-action-row">
        <a-input-password v-model="draft.auth.apiKey" placeholder="sk-..." />
        <a-button :disabled="!draft.auth.apiKey.trim()" @click="emit('copy-api-key')">
          <template #icon><icon-copy /></template>
        </a-button>
      </div>
      <template #extra>API 密钥不支持签到，仅支持查询该密钥自身额度。</template>
    </a-form-item>
  </template>
</template>
