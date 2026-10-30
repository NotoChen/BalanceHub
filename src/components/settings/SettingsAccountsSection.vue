<script setup lang="ts">
import { inject } from "vue";
import { IconUserGroup } from "@arco-design/web-vue/es/icon";
import { LOGIN_ACCOUNTS_CONTEXT } from "../../composables/useLoginAccounts";
import { BROWSER_RUNTIME_CONTEXT } from "../../composables/useBrowserRuntime";

const loginAccounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const browserRuntime = inject(BROWSER_RUNTIME_CONTEXT);
</script>

<template>
  <div class="settings-page settings-accounts-page">
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconUserGroup /></span>
        <div>
          <strong>登录账号与授权</strong>
          <span>需要登录的站点复用这里的账号，不必逐站重新输入。</span>
        </div>
      </header>
      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>Linux DO / GitHub 等登录账号</strong>
          <span>独立保存多个账号，查看 Cookie 和关联站点</span>
        </div>
        <a-button @click="loginAccounts?.open()">登录账号管理</a-button>
      </div>
      <div v-if="browserRuntime" class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>浏览器登录与验证（可选）</strong>
          <span>{{ browserRuntime.state.value?.ready ? `当前使用 ${browserRuntime.state.value.browser?.name || '已安装浏览器'}，可管理或切换组件` : '使用本机兼容浏览器或独立 Chromium，首次使用时确认安装' }}</span>
        </div>
        <a-button @click="browserRuntime.open">浏览器组件</a-button>
      </div>
    </section>
  </div>
</template>
