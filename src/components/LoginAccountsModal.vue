<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { Modal } from "@arco-design/web-vue";
import { IconPlus, IconRefresh, IconUserGroup } from "@arco-design/web-vue/es/icon";
import { LOGIN_ACCOUNTS_CONTEXT, LOGIN_PLATFORM_OPTIONS } from "../composables/useLoginAccounts";
import { BROWSER_RUNTIME_CONTEXT } from "../composables/useBrowserRuntime";
import { PROVIDER_CREDENTIALS_CONTEXT } from "../composables/useProviderCredentials";
import { readLoginCookie } from "../api/login-accounts";
import type { LoginPlatform } from "../stores/provider-types";
import CredentialSecret from "./CredentialSecret.vue";

const accounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const credentials = inject(PROVIDER_CREDENTIALS_CONTEXT);
const runtime = inject(BROWSER_RUNTIME_CONTEXT);
const current = computed(() => accounts?.selected.value ?? null);
const name = ref("");
const platform = ref<LoginPlatform>("linuxDo");
const adding = ref(false);
const cookiesOpen = ref(false);
const cookieScope = ref(0);
watch(current, (value) => {
  name.value = value?.name ?? ""; platform.value = value?.platform ?? "linuxDo";
  if (value) adding.value = false;
  cookiesOpen.value = false; cookieScope.value++;
});
watch(() => accounts?.visible.value, () => { adding.value = false; cookiesOpen.value = false; cookieScope.value++; });
const formatTime = (value: number | null) => value ? new Date(value).toLocaleString("zh-CN", { hour12: false }) : "尚无记录";

function add() { if (!accounts) return; accounts.selectedId.value = ""; name.value = ""; platform.value = "linuxDo"; adding.value = true; }
function remove(removeEntry: boolean) {
  const expectedId = current.value?.id;
  Modal.confirm({ title: removeEntry ? "删除这个登录账号" : "清除这个账号的本地登录状态",
    content: removeEntry ? "删除该账号的本地浏览器环境并解除关联。已导入的站点凭据保留；平台端的授权不会撤销。" : "仅清除这个账号的 Cookie 和浏览器存储。账号身份、历史记录和站点关联保留，下次需重新登录同一账号。其他账号及各站点凭据保留。",
    okText: removeEntry ? "删除本地账号" : "清除本地登录", cancelText: "取消", onOk: () => {
      if (accounts?.visible.value && expectedId === current.value?.id) void accounts.remove(removeEntry);
    } });
}
function inspect() { cookiesOpen.value = true; cookieScope.value++; void accounts?.inspectCookies(); }
function viewProvider(id: string) { if (accounts) accounts.visible.value = false; credentials?.open(id); }
</script>

<template>
  <a-modal v-if="accounts && !accounts.choosing.value" v-model:visible="accounts.visible.value" :width="850" :footer="false" :unmount-on-close="true" modal-class="surface-modal">
    <template #title><span class="login-accounts-title"><IconUserGroup /> 登录账号管理</span></template>
    <div v-if="accounts.visible.value" class="login-accounts">
      <p class="login-accounts-intro">管理平台身份、登录状态和关联中转站。每个账号使用独立的登录环境。</p>
      <div v-if="runtime" class="login-browser-runtime">
        <div><strong>浏览器登录与验证</strong><span>{{ runtime.state.value?.ready ? `当前使用 ${runtime.state.value.browser?.name || '已安装浏览器'}` : '可选组件，使用浏览器登录或站点验证时安装' }}</span></div>
        <a-button size="small" @click="runtime.open">浏览器组件</a-button>
      </div>
      <a-alert v-if="accounts.error.value" type="error">{{ accounts.error.value }}</a-alert>
      <div class="login-accounts-layout">
        <aside class="login-account-list">
          <div class="login-account-list-actions">
            <a-button type="primary" size="small" @click="add"><template #icon><IconPlus /></template>新增账号</a-button>
            <a-button size="small" aria-label="刷新登录账号" :loading="accounts.pending.value === 'list'" @click="accounts.reload"><IconRefresh /></a-button>
          </div>
          <button v-for="account in accounts.accounts.value" :key="account.id" type="button" class="login-account-option"
            :class="{ selected: current?.id === account.id }" :aria-label="account.name" :aria-pressed="current?.id === account.id" @click="accounts.selectedId.value = account.id">
            <strong>{{ account.identity || account.name }}</strong>
            <span>{{ account.platformLabel }} · {{ account.name }} · {{ account.linkedProviders.length }} 个站点</span>
            <small>{{ account.sessionLabel }}</small>
          </button>
          <p v-if="!accounts.accounts.value.length" class="login-muted">尚未保存登录账号</p>
        </aside>

        <main class="login-account-detail">
          <template v-if="current || adding || !accounts.accounts.value.length">
            <div class="login-account-form">
              <label for="login-account-name">账号备注</label>
              <a-input v-model="name" :input-attrs="{ id: 'login-account-name', 'aria-label': '账号备注' }" placeholder="例如：Linux DO A / GitHub 工作账号" :max-length="80" />
              <label for="login-account-platform">登录平台</label>
              <a-select id="login-account-platform" v-model="platform" :options="LOGIN_PLATFORM_OPTIONS" :disabled="Boolean(current?.identity)" />
              <a-button :loading="accounts.pending.value === 'save'" :disabled="!name.trim() || Boolean(current?.busy)" @click="accounts.save(name, platform, !current)">{{ current ? '保存备注' : '保存账号' }}</a-button>
            </div>
            <template v-if="current">
              <a-alert v-if="current.sessionProblem" type="error">{{ current.sessionProblem }}。可清除本地登录状态后重新登录。</a-alert>
              <dl class="login-account-facts">
                <div><dt>平台身份</dt><dd>{{ current.identity || '未读取到平台身份，以上名称为本地备注' }}</dd></div>
                <div><dt>登录状态</dt><dd>{{ current.sessionLabel }}</dd></div>
                <div v-if="current.identityObservedAt"><dt>最近识别身份</dt><dd>{{ formatTime(current.identityObservedAt) }}</dd></div>
                <div><dt>最近用于站点</dt><dd>{{ formatTime(current.lastUsedAt) }}</dd></div>
              </dl>
              <div class="login-account-actions">
                <a-button :disabled="!current.canOpen || current.busy || Boolean(accounts.pending.value)" @click="accounts.launch(false)">登录 / 打开账号</a-button>
                <a-button :disabled="!current.canOpen || current.busy || Boolean(accounts.pending.value)" @click="accounts.launch(true)">平台授权管理</a-button>
                <a-button :loading="accounts.pending.value === 'cookies'" @click="inspect">查看 Cookie（{{ current.cookieCount }}）</a-button>
              </div>
              <p class="login-muted">登录有效性以平台当前页面为准。授权管理将在此账号的独立窗口中打开。</p>
              <section class="login-linked">
                <strong>关联的中转站</strong>
                <p v-if="!current.linkedProviders.length" class="login-muted">还没有通过此账号导入的站点。</p>
                <button v-for="provider in current.linkedProviders" :key="provider.id" type="button" class="login-linked-provider" @click="viewProvider(provider.id)">{{ provider.name }} <span>查看凭据 →</span></button>
              </section>
              <section v-if="cookiesOpen" class="login-cookies">
                <p class="login-muted">这里是此登录环境保存的 Cookie。站点 JWT 与续期凭据在对应站点的“凭据详情”中管理。</p>
                <p v-if="!accounts.cookies.value.length" class="login-muted">没有可显示的 Cookie。</p>
                <div v-for="cookie in accounts.cookies.value" :key="cookie.id" class="login-cookie-row">
                  <strong>{{ cookie.name }}</strong>
                  <span class="login-muted">{{ cookie.domain }}{{ cookie.path }} · {{ cookie.httpOnly ? 'HttpOnly · ' : '' }}{{ cookie.secure ? 'Secure · ' : '' }}{{ cookie.expires > 0 ? formatTime(cookie.expires * 1000) : '会话 Cookie' }}</span>
                  <CredentialSecret :scope="`${current.id}:${current.generation}:${cookieScope}:${cookie.id}`" :label="cookie.name" :read="() => readLoginCookie(current!.id, cookie.id)" />
                </div>
              </section>
              <div class="login-account-remove">
                <a-button size="small" :disabled="current.busy || Boolean(accounts.pending.value)" @click="remove(false)">清除本地登录</a-button>
                <a-button size="small" status="danger" :disabled="current.busy || Boolean(accounts.pending.value)" @click="remove(true)">删除账号</a-button>
              </div>
            </template>
            <p v-else class="login-muted">保存后可在左侧选中此账号。首次登录时输入该账号的凭据，以后直接复用它的登录状态。</p>
          </template>
          <a-empty v-else description="选择已有账号，或新增另一个账号" />
        </main>
      </div>
    </div>
  </a-modal>
</template>

<style scoped>
.login-accounts-title { display: flex; gap: 8px; align-items: center; }
.login-accounts { display: grid; gap: 14px; color: var(--color-text-1); }
.login-accounts-intro { margin: 0; color: var(--color-text-2); font-size: 13px; line-height: 1.7; }
.login-accounts-layout { display: grid; grid-template-columns: 218px minmax(0,1fr); min-height: 370px; max-height: 62vh; }
.login-account-list { display: flex; flex-direction: column; gap: 8px; padding-right: 16px; border-right: 1px solid var(--color-border-2); overflow-y: auto; }
.login-account-list-actions { display: flex; gap: 8px; padding-bottom: 7px; }
.login-account-option { display: grid; gap: 6px; padding: 12px; text-align: left; border: 1px solid var(--color-border-2); border-radius: 7px; background: var(--color-bg-2); color: var(--color-text-1); cursor: pointer; }
.login-account-option.selected { border-color: rgb(var(--primary-6)); background: var(--color-primary-light-1); }
.login-account-option strong { overflow-wrap: anywhere; font-size: 13px; }
.login-account-option span,.login-account-option small { font-size: 11px; line-height: 1.5; color: var(--color-text-3); }
.login-account-detail { overflow-y: auto; padding: 1px 4px 1px 22px; min-width: 0; }
.login-account-form { display: grid; grid-template-columns: 68px minmax(0,1fr); align-items: center; gap: 12px 10px; font-size: 12px; }
.login-account-form > button { grid-column: 2; justify-self: start; }
.login-account-facts { display: grid; gap: 9px; margin: 20px 0; font-size: 12px; }
.login-account-facts div { display: grid; grid-template-columns: 86px minmax(0,1fr); gap: 8px; }
.login-account-facts dt { color: var(--color-text-3); }
.login-account-facts dd { margin: 0; overflow-wrap: anywhere; }
.login-account-actions { display: flex; flex-wrap: wrap; gap: 8px; }
.login-muted { color: var(--color-text-3); font-size: 12px; line-height: 1.7; }
.login-linked { margin-top: 19px; padding-top: 16px; border-top: 1px solid var(--color-border-2); font-size: 13px; }
.login-linked-provider { display: flex; width: 100%; justify-content: space-between; align-items: center; gap: 12px; padding: 9px 0; text-align: left; background: none; border: 0; color: var(--color-text-1); cursor: pointer; }
.login-linked-provider span { flex-shrink: 0; color: rgb(var(--primary-6)); font-size: 12px; }
.login-account-remove { display: flex; gap: 10px; margin-top: 24px; }
.login-cookies { border-top: 1px solid var(--color-border-2); margin-top: 18px; }
.login-cookie-row { display: grid; gap: 7px; padding: 12px 0; border-bottom: 1px solid var(--color-border-2); overflow-wrap: anywhere; font-size: 12px; }
.login-browser-runtime { display: flex; justify-content: space-between; align-items: center; gap: 16px; padding: 12px; border: 1px solid var(--color-border-2); border-radius: 7px; }
.login-browser-runtime > div { display: grid; gap: 5px; font-size: 12px; }
.login-browser-runtime span { color: var(--color-text-3); }
</style>
