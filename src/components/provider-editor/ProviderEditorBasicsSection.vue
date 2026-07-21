<script setup lang="ts">
import type { ProviderInput } from "../../stores/providers";
import { IconDelete, IconPlus } from "@arco-design/web-vue/es/icon";
import { authModeOptions } from "./options";

defineProps<{
  draft: ProviderInput;
}>();

function addBackupUrl(draft: ProviderInput) {
  draft.identity.backupUrls.push("");
}

function removeBackupUrl(draft: ProviderInput, index: number) {
  draft.identity.backupUrls.splice(index, 1);
}
</script>

<template>
  <a-form-item field="identity.baseUrl" label="中转站地址" required>
    <a-input v-model="draft.identity.baseUrl" placeholder="https://relay.example.com" />
  </a-form-item>
  <a-form-item field="identity.backupUrls" label="备用地址">
    <div class="provider-backup-url-list">
      <div
        v-for="(_, index) in draft.identity.backupUrls"
        :key="`backup-url-${index}`"
        class="provider-backup-url-row"
      >
        <a-input
          v-model="draft.identity.backupUrls[index]"
          :placeholder="`备用地址 ${index + 1}`"
        />
        <a-button
          type="text"
          status="danger"
          aria-label="删除备用地址"
          @click="removeBackupUrl(draft, index)"
        >
          <template #icon><icon-delete /></template>
        </a-button>
      </div>
      <a-button type="outline" size="small" @click="addBackupUrl(draft)">
        <template #icon><icon-plus /></template>
        添加备用地址
      </a-button>
    </div>
    <template #extra>仅保存维护信息，当前不会自动切换或参与请求。</template>
  </a-form-item>
  <a-form-item field="cli.preferredModel" label="首选模型">
    <a-input v-model="draft.cli.preferredModel" placeholder="例如 claude-sonnet-4-20250514" />
    <template #extra>仅保存维护信息，后续临时 CLI 接入时使用。</template>
  </a-form-item>
  <a-form-item field="auth.mode" label="优先认证方式" required>
    <a-select v-model="draft.auth.mode" :options="authModeOptions" />
    <template #extra>用于决定请求时优先使用哪种凭据，不会隐藏其他已配置凭据。</template>
  </a-form-item>
</template>
