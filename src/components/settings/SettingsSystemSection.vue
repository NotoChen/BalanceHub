<script setup lang="ts">
import { inject } from "vue";
import { LOGIN_ACCOUNTS_CONTEXT } from "../../composables/useLoginAccounts";
import { BROWSER_RUNTIME_CONTEXT } from "../../composables/useBrowserRuntime";
import {
  IconDownload,
  IconStorage,
  IconUpload,
  IconWifi,
  IconUserGroup,
} from "@arco-design/web-vue/es/icon";
import type { AppSettings } from "../../stores/providers";
import { proxyModeOptions } from "../../utils/proxy-options";
import SettingsCloudSyncSection from "./SettingsCloudSyncSection.vue";

defineProps<{
  settings: AppSettings;
  exportingAppData: boolean;
  importingAppData: boolean;
}>();

const emit = defineEmits<{
  "export-app-data": [];
  "import-app-data": [];
}>();

const loginAccounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const browserRuntime = inject(BROWSER_RUNTIME_CONTEXT);
</script>

<template>
  <div class="settings-page settings-system-page">
    <SettingsCloudSyncSection />
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconWifi /></span>
        <div>
          <strong>网络代理</strong>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>代理策略</strong>
          </div>
          <a-select v-model="settings.proxyMode" :options="proxyModeOptions" />
        </div>
        <div v-if="settings.proxyMode === 'custom'" class="settings-setting-row settings-setting-row-wide">
          <div class="settings-setting-copy">
            <strong>代理地址</strong>
          </div>
          <a-input
            v-model="settings.proxyUrl"
            placeholder="http://127.0.0.1:6152"
            title="支持 HTTP、HTTPS 和 SOCKS5 地址"
          />
        </div>
      </div>
    </section>
    <section class="settings-card">
      <header class="settings-card-header"><span class="settings-card-icon"><IconUserGroup /></span><div><strong>登录账号与授权</strong></div></header>
      <div class="settings-setting-row">
        <div class="settings-setting-copy"><strong>Linux DO / GitHub 等登录账号</strong><span>独立保存多个账号，查看 Cookie 和关联站点</span></div>
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

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconStorage /></span>
        <div>
          <strong>数据备份</strong>
        </div>
      </header>

      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>导出当前配置</strong>
          <span>备份中转站、认证凭据和应用设置，用于迁移或恢复。</span>
        </div>
          <a-button :loading="exportingAppData" :disabled="importingAppData" @click="emit('export-app-data')">
            <template #icon><IconDownload /></template>
            导出备份
          </a-button>
      </div>
      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>从备份恢复</strong>
          <span>选择备份文件，确认后替换当前中转站和应用设置。</span>
        </div>
          <a-button :loading="importingAppData" :disabled="exportingAppData" @click="emit('import-app-data')">
            <template #icon><IconUpload /></template>
            选择备份
          </a-button>
      </div>
    </section>
  </div>
</template>
