<script setup lang="ts">
import { computed, inject } from "vue";
import { IconLock, IconRefresh } from "@arco-design/web-vue/es/icon";
import { PROVIDER_CREDENTIALS_CONTEXT } from "../composables/useProviderCredentials";
import { LOGIN_ACCOUNTS_CONTEXT } from "../composables/useLoginAccounts";
import { readProviderCredential } from "../api/login-accounts";
import CredentialSecret from "./CredentialSecret.vue";
import { formatProviderSyncTime } from "../utils/provider-display";
const credentials = inject(PROVIDER_CREDENTIALS_CONTEXT);
const accounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const details = computed(() => credentials?.details.value ?? null);
const time = (value: number | string | null) => {
  if (!value) return '尚无记录';
  const parsed = typeof value === 'string' && /^\d+$/.test(value) ? Number(value) : value;
  const date = new Date(parsed);
  return Number.isNaN(date.getTime()) ? '尚无记录' : date.toLocaleString('zh-CN', { hour12: false });
};
function manageAccount() {
  const id = details.value?.binding?.accountId ?? undefined;
  if (credentials) credentials.visible.value = false;
  accounts?.open(id);
}
</script>

<template>
  <a-modal v-if="credentials" v-model:visible="credentials.visible.value" :width="730" :footer="false" :unmount-on-close="true" modal-class="surface-modal">
    <template #title><span class="provider-credentials-title"><IconLock /> 凭据详情</span></template>
    <div v-if="credentials.visible.value" class="provider-credentials-content">
      <a-alert v-if="credentials.error.value" type="error">{{ credentials.error.value }}</a-alert>
      <a-spin v-if="!details && credentials.pending.value" />
      <template v-if="details">
        <div class="provider-credentials-heading"><strong>{{ details.providerName }}</strong><a-button size="small" :loading="credentials.pending.value === 'load'" @click="credentials.refresh"><template #icon><IconRefresh /></template>刷新详情</a-button></div>
        <dl class="provider-credentials-facts">
          <div><dt>登录账号</dt><dd>{{ details.accountName || (details.binding ? '原登录账号已移除' : '尚未关联登录账号') }} <a-button type="text" size="mini" @click="manageAccount">管理登录账号</a-button></dd></div>
          <div><dt>同步认证方式</dt><dd>{{ details.authenticationLabel }}</dd></div>
          <div><dt>凭据最近更新</dt><dd>{{ time(details.updatedAt) }}</dd></div>
          <div><dt>上次同步成功</dt><dd>{{ formatProviderSyncTime(details.verifiedAt) || '尚无记录' }}</dd></div>
        </dl>
        <div class="provider-validation-scope"><strong>本次验证范围</strong><p>{{ details.validationScope }}</p><p>结果仅覆盖本次站点请求；其他已保存凭据和平台登录状态仍需分别确认。</p></div>
        <a-alert v-if="details.error && !credentials.error.value" type="warning">{{ details.error }}</a-alert>
        <div class="provider-credentials-actions">
          <a-button type="primary" :disabled="!details.canValidate || Boolean(credentials.pending.value)" :loading="credentials.pending.value === 'validate'" @click="credentials.validate">{{ details.validationLabel }}</a-button>
          <a-button v-if="details.canLogin" :disabled="Boolean(credentials.pending.value)" @click="credentials.login">重新登录 / 授权</a-button>
        </div>
        <p class="provider-credentials-note">以下是此站点实际保存的凭据。站点 JWT 不等于 Linux DO / GitHub 的 OAuth Token；平台 Cookie 在登录账号中管理。</p>
        <div class="provider-credential-entries">
          <a-empty v-if="!details.entries.length" description="此站点没有保存的凭据" />
          <section v-for="entry in details.entries" :key="entry.kind" class="provider-credential-entry">
            <header><strong>{{ entry.label }}</strong><span>{{ entry.status }}</span></header>
            <p>{{ entry.source }}<template v-if="entry.expiresAt"> · 到期 {{ time(entry.expiresAt * 1000) }}</template></p>
            <CredentialSecret :scope="`${details.providerId}:${credentials.secretScope.value}:${entry.kind}`" :label="entry.label" :read="() => readProviderCredential(details!.providerId, entry.kind, details!.credentialRevision)" />
            <a-button v-if="entry.clearLabel" type="text" status="danger" size="mini" :disabled="Boolean(credentials.pending.value)" @click="credentials.clear(entry.kind)">{{ entry.clearLabel }}</a-button>
          </section>
        </div>
      </template>
    </div>
  </a-modal>
</template>

<style scoped>
.provider-credentials-title { display: flex; align-items: center; gap: 8px; }
.provider-credentials-content { display: grid; gap: 14px; color: var(--color-text-1); }
.provider-credentials-heading { display: flex; justify-content: space-between; align-items: center; gap: 15px; }
.provider-credentials-facts { display: grid; gap: 8px; margin: 0; font-size: 12px; }
.provider-credentials-facts div { display: grid; grid-template-columns: 100px minmax(0,1fr); align-items: center; }
.provider-credentials-facts dt { color: var(--color-text-3); }
.provider-credentials-facts dd { margin: 0; overflow-wrap: anywhere; }
.provider-credentials-actions { display: flex; gap: 10px; }
.provider-credentials-note { margin: 0; font-size: 12px; line-height: 1.7; color: var(--color-text-3); }
.provider-validation-scope { display: grid; gap: 6px; padding: 12px; border-radius: 7px; background: var(--color-fill-1); font-size: 12px; }
.provider-validation-scope p { margin: 0; color: var(--color-text-2); line-height: 1.7; }
.provider-credential-entries { display: grid; gap: 12px; max-height: 47vh; overflow-y: auto; padding-right: 5px; }
.provider-credential-entry { border: 1px solid var(--color-border-2); border-radius: 8px; padding: 14px; display: grid; gap: 10px; min-width: 0; }
.provider-credential-entry header { display: flex; justify-content: space-between; align-items: center; gap: 14px; font-size: 13px; }
.provider-credential-entry header span { font-size: 11px; color: var(--color-text-3); }
.provider-credential-entry p { margin: 0; color: var(--color-text-3); font-size: 12px; line-height: 1.6; }
.provider-credential-entry > button { justify-self: start; }
</style>
