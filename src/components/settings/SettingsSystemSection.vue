<script setup lang="ts">
import {
  IconDownload,
  IconRefresh,
  IconStorage,
  IconUpload,
  IconWifi,
} from "@arco-design/web-vue/es/icon";
import type { AppSettings, ProxyMode } from "../../stores/providers";

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

function versionLabel(value: unknown) {
  if (typeof value === "string" && value.trim()) {
    return `v${value.trim()}`;
  }
  return "开发环境";
}
</script>

<template>
  <div class="settings-page settings-system-page">
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
        <span class="settings-version-badge">{{ versionLabel(appVersion) }}</span>
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
