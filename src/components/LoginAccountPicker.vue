<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { IconPlus, IconRefresh, IconUserGroup } from "@arco-design/web-vue/es/icon";
import { LOGIN_ACCOUNTS_CONTEXT, LOGIN_PLATFORM_OPTIONS } from "../composables/useLoginAccounts";
import type { LoginPlatform } from "../stores/provider-types";

const accounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const selection = computed(() => accounts?.selectionView.value);
const target = computed(() => selection.value?.target);
const adding = ref(false);
const name = ref("");
const platform = ref<LoginPlatform>("linuxDo");
watch(target, () => { adding.value = false; name.value = ""; platform.value = "linuxDo"; });
const creating = computed(() => selection.value?.creating ?? false);
</script>

<template>
  <a-modal v-if="accounts && accounts.choosing.value" v-model:visible="accounts.visible.value" :width="620" :footer="false" :unmount-on-close="true" modal-class="surface-modal">
    <template #title><span class="login-picker-title"><IconUserGroup /> 选择登录账号</span></template>
    <div class="login-picker">
      <div class="login-picker-target"><span>登录中转站</span><strong>{{ target?.name }}</strong><p>{{ target?.baseUrl }}</p></div>
      <a-alert v-if="accounts.error.value || selection?.error" type="error">{{ accounts.error.value || selection?.error }}</a-alert>
      <template v-if="!adding">
        <p class="login-picker-note">选择要使用的平台身份。完成站点登录或授权后，将自动导入到这个中转站。</p>
        <div class="login-picker-list">
          <button v-for="account in accounts.accounts.value" :key="account.id" type="button" class="login-picker-account"
            :class="{ selected: account.id === accounts.selectedId.value }" :disabled="!account.canLogin"
            :aria-pressed="account.id === accounts.selectedId.value"
            @click="accounts.selectedId.value = account.id">
            <span class="login-picker-account-heading"><strong>{{ account.identity || account.name }}</strong><small v-if="account.id === target?.previousAccountId">此站点上次使用</small></span>
            <span>{{ account.platformLabel }} · {{ account.identity ? account.name : '本地备注，身份尚未识别' }}</span>
            <span class="login-picker-note">{{ account.sessionLabel }}</span>
          </button>
          <a-empty v-if="!accounts.accounts.value.length && !accounts.pending.value" description="添加一个账号，开始首次登录" />
          <a-spin v-if="accounts.pending.value === 'list'" />
        </div>
        <div class="login-picker-footer">
          <div class="login-picker-secondary-actions">
            <a-button @click="adding = true"><template #icon><IconPlus /></template>使用新账号</a-button>
            <a-button aria-label="刷新可选登录账号" :loading="accounts.pending.value === 'list'" @click="accounts.reload"><IconRefresh /></a-button>
          </div>
          <a-button type="primary" :disabled="!accounts.selected.value?.canLogin || Boolean(accounts.pending.value)" @click="accounts.confirmChoice">使用此账号登录</a-button>
        </div>
      </template>
      <template v-else>
        <p class="login-picker-note">新账号会使用独立登录环境。创建后直接打开上面的站点，完成首次登录即可导入。</p>
        <div class="login-picker-form">
          <label for="login-picker-name">账号备注</label>
          <a-input v-model="name" :input-attrs="{ id: 'login-picker-name', 'aria-label': '新登录账号备注' }" placeholder="例如：Linux DO 工作账号" :max-length="80" :disabled="creating" />
          <label for="login-picker-platform">登录平台</label>
          <a-select id="login-picker-platform" v-model="platform" :options="LOGIN_PLATFORM_OPTIONS" :disabled="creating" />
        </div>
        <div class="login-picker-footer">
          <a-button :disabled="creating" @click="adding = false">返回账号列表</a-button>
          <a-button type="primary" :loading="creating" :disabled="!name.trim()" @click="accounts.createAndChoose(name, platform)">创建并登录此站点</a-button>
        </div>
      </template>
    </div>
  </a-modal>
</template>

<style scoped>
.login-picker-title { display: flex; align-items: center; gap: 8px; }
.login-picker { display: grid; gap: 16px; color: var(--color-text-1); }
.login-picker-target { display: grid; gap: 5px; padding-bottom: 15px; border-bottom: 1px solid var(--color-border-2); }
.login-picker-target > span { font-size: 12px; color: var(--color-text-3); }
.login-picker-target strong { font-size: 15px; overflow-wrap: anywhere; }
.login-picker-target p { margin: 0; font-size: 12px; color: var(--color-text-2); overflow-wrap: anywhere; }
.login-picker-note { margin: 0; font-size: 12px; line-height: 1.7; color: var(--color-text-3); }
.login-picker-list { display: grid; gap: 10px; max-height: 42vh; overflow-y: auto; }
.login-picker-account { display: grid; gap: 7px; padding: 14px; border: 1px solid var(--color-border-2); border-radius: 7px; background: var(--color-bg-2); color: var(--color-text-1); text-align: left; cursor: pointer; font-size: 12px; overflow-wrap: anywhere; }
.login-picker-account.selected { border-color: rgb(var(--primary-6)); background: var(--color-primary-light-1); }
.login-picker-account:disabled { cursor: not-allowed; opacity: .6; }
.login-picker-account-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.login-picker-account-heading strong { font-size: 14px; }
.login-picker-account-heading small { color: rgb(var(--primary-6)); font-size: 11px; flex-shrink: 0; }
.login-picker-footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-top: 14px; border-top: 1px solid var(--color-border-2); }
.login-picker-secondary-actions { display: flex; align-items: center; gap: 8px; }
.login-picker-form { display: grid; grid-template-columns: 68px minmax(0, 1fr); align-items: center; gap: 14px 12px; font-size: 12px; }
</style>
