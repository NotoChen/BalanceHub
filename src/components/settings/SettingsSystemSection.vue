<script setup lang="ts">
import { inject } from "vue";
import { LOGIN_ACCOUNTS_CONTEXT } from "../../composables/useLoginAccounts";
import { BROWSER_RUNTIME_CONTEXT } from "../../composables/useBrowserRuntime";
import {
  IconDownload,
  IconRefresh,
  IconStorage,
  IconUpload,
  IconWifi,
  IconUserGroup,
} from "@arco-design/web-vue/es/icon";
import type { AppSettings, ProxyMode } from "../../stores/providers";
import { formatAppVersionLabel } from "../../utils/app-version";

interface SelectOption<T extends string = string> {
  label: string;
  value: T;
}

defineProps<{
  settings: AppSettings;
  expanded?: boolean;
  exportingAppData: boolean;
  importingAppData: boolean;
  appVersion: string;
  checkingForUpdate: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
  "export-app-data": [];
  "import-app-data": [];
  "check-for-update": [];
}>();

const proxyModeOptions: SelectOption<ProxyMode>[] = [
  { label: "跟随系统代理", value: "system" },
  { label: "不使用代理", value: "noProxy" },
  { label: "自定义代理", value: "custom" },
];
const loginAccounts = inject(LOGIN_ACCOUNTS_CONTEXT);
const browserRuntime = inject(BROWSER_RUNTIME_CONTEXT);

</script>

<template>
  <div class="settings-page settings-system-page">
    <section class="settings-card">
      <header class="settings-card-header"><span class="settings-card-icon"><IconUserGroup /></span><div><strong>登录账号与授权</strong></div></header>
      <div class="settings-setting-row settings-setting-row-action">
        <div class="settings-setting-copy"><strong>Linux DO / GitHub 等登录账号</strong><span>独立保存多个账号，查看 Cookie 和关联站点</span></div>
        <a-button @click="loginAccounts?.open()">登录账号管理</a-button>
      </div>
      <div v-if="browserRuntime" class="settings-setting-row settings-setting-row-action">
        <div class="settings-setting-copy">
          <strong>浏览器登录与验证（可选）</strong>
          <span>{{ browserRuntime.state.value?.ready ? `当前使用 ${browserRuntime.state.value.browser?.name || '已安装浏览器'}，可管理或切换组件` : '使用本机兼容浏览器或独立 Chromium，首次使用时确认安装' }}</span>
        </div>
        <a-button @click="browserRuntime.open">浏览器组件</a-button>
      </div>
    </section>
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconWifi /></span>
        <div>
          <strong>网络代理</strong>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row">
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
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-green"><IconRefresh /></span>
        <div>
          <strong>版本更新</strong>
        </div>
        <span class="settings-version-badge">{{ formatAppVersionLabel(appVersion) }}</span>
      </header>

      <div class="settings-setting-row settings-setting-row-action">
        <div class="settings-setting-copy">
          <strong>检查新版本</strong>
        </div>
        <a-button :loading="checkingForUpdate" @click="emit('check-for-update')">
          <template #icon><IconRefresh /></template>
          检查更新
        </a-button>
      </div>
    </section>

    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconStorage /></span>
        <div>
          <strong>配置文件</strong>
        </div>
      </header>

      <div class="settings-setting-row settings-setting-row-action">
        <div class="settings-setting-copy">
          <strong>导入与导出</strong>
        </div>
        <a-space>
          <a-button :loading="exportingAppData" @click="emit('export-app-data')">
            <template #icon><IconDownload /></template>
            导出
          </a-button>
          <a-button :loading="importingAppData" @click="emit('import-app-data')">
            <template #icon><IconUpload /></template>
            导入
          </a-button>
        </a-space>
      </div>
    </section>
  </div>
</template>
